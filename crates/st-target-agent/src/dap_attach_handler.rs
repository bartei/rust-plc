//! In-process DAP handler for attach-to-running-engine debug sessions.
//!
//! Translates DAP protocol messages (Content-Length framed JSON over TCP)
//! to `DebugCommand`/`DebugResponse` on the channels provided by
//! `RuntimeManager::debug_attach()`. No subprocess is spawned — the
//! debugger controls the same VM that was already running.
//!
//! Architecture: a reader thread parses DAP messages from TCP and pushes
//! them onto a channel. The main loop selects between TCP messages and
//! engine events, forwarding in both directions without blocking.

use crate::server::AppState;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Messages from the reader thread to the main loop.
enum Input {
    DapMessage(serde_json::Value),
    DapDisconnected,
    EngineEvent(st_engine::DebugResponse),
    EngineDisconnected,
}

/// Bidirectional path mapper for DAP source path remapping.
///
/// Translates between client-side paths (e.g., `/home/user/project/main.st`)
/// and target-side paths (e.g., `/var/lib/st-plc/programs/current_source/main.st`).
/// Follows the `localRoot`/`remoteRoot` pattern used by Node.js and Python debug adapters.
struct PathMapper {
    /// Client-side workspace root (from attach args `localRoot`).
    local_root: Option<String>,
    /// Target-side source directory (the `source_dir` parameter).
    remote_root: String,
}

impl PathMapper {
    fn new(remote_root: &Path) -> Self {
        let s = remote_root.to_string_lossy().to_string();
        Self {
            local_root: None,
            remote_root: s.trim_end_matches('/').to_string(),
        }
    }

    /// Configure the client-side root from the attach request arguments.
    fn set_local_root(&mut self, local_root: String) {
        let trimmed = local_root
            .trim_end_matches('/')
            .trim_end_matches('\\');
        self.local_root = Some(trimmed.to_string());
    }

    /// Remap a target-side path to a client-side path.
    /// Used for stackTrace responses (outgoing to VS Code).
    fn to_local(&self, remote_path: &str) -> String {
        let Some(ref local_root) = self.local_root else {
            return remote_path.to_string();
        };
        let prefix = format!("{}/", self.remote_root);
        if let Some(rel) = remote_path.strip_prefix(&prefix) {
            format!("{local_root}/{rel}")
        } else {
            remote_path.to_string()
        }
    }

    /// Remap a client-side path to a target-side path.
    /// Used for setBreakpoints requests (incoming from VS Code).
    fn to_remote(&self, local_path: &str) -> String {
        let Some(ref local_root) = self.local_root else {
            return local_path.to_string();
        };
        // Normalize Windows backslashes for comparison
        let normalized = local_path.replace('\\', "/");
        let prefix = format!("{local_root}/");
        if let Some(rel) = normalized.strip_prefix(&prefix) {
            format!("{}/{}", self.remote_root, rel)
        } else {
            local_path.to_string()
        }
    }
}

/// Source map for multi-file debugging: tracks virtual byte offsets and
/// function-to-file mappings, replicating the logic from `st-dap/server.rs`.
///
/// The compiler concatenates stdlib + project files into a virtual source space.
/// Source-map entries in the compiled module use virtual offsets, so we need to
/// know each file's base offset to convert between virtual ↔ file-local positions.
struct SourceMap {
    /// file_path → virtual byte offset in the concatenated source.
    file_virtual_offsets: HashMap<String, usize>,
    /// function_name (UPPERCASE) → (file_path, file_content).
    func_to_file: HashMap<String, (String, String)>,
}

impl SourceMap {
    /// Build the source map from the source directory on disk.
    ///
    /// Uses `discover_project` + `load_project_sources` to load files in the
    /// SAME order as the compiler (sorted, respecting plc-project.yaml). This is
    /// critical: virtual offsets depend on file order, and a mismatch means all
    /// breakpoints and line numbers are wrong.
    fn build(source_dir: &Path) -> Self {
        // Load files using the same project discovery as the compiler
        let project_files = match st_syntax::project::discover_project(Some(source_dir)) {
            Ok(project) => match st_syntax::project::load_project_sources(&project) {
                Ok(sources) => sources
                    .into_iter()
                    .map(|(p, c)| (p.to_string_lossy().to_string(), c))
                    .collect::<Vec<_>>(),
                Err(e) => {
                    warn!("Source map: failed to load project sources: {e}");
                    Vec::new()
                }
            },
            Err(e) => {
                warn!("Source map: failed to discover project: {e}");
                Vec::new()
            }
        };

        // Compute stdlib total size (same files used by the compiler)
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let stdlib_len: usize = stdlib.iter().map(|s| s.len()).sum();

        // Compute virtual offsets in the SAME order as parse_multi
        let mut file_virtual_offsets = HashMap::new();
        let mut cumulative = stdlib_len;
        for (path, content) in &project_files {
            file_virtual_offsets.insert(path.clone(), cumulative);
            cumulative += content.len();
        }

        // Build function → source file mapping by parsing each file
        let mut func_to_file: HashMap<String, (String, String)> = HashMap::new();
        for (path, content) in &project_files {
            let parse = st_syntax::parse(content);
            for item in &parse.source_file.items {
                let names: Vec<String> = match item {
                    st_syntax::ast::TopLevelItem::Program(p) =>
                        vec![p.name.name.clone()],
                    st_syntax::ast::TopLevelItem::Function(f) =>
                        vec![f.name.name.clone()],
                    st_syntax::ast::TopLevelItem::FunctionBlock(fb) =>
                        vec![fb.name.name.clone()],
                    st_syntax::ast::TopLevelItem::Class(cls) => {
                        let mut v = vec![cls.name.name.clone()];
                        for m in &cls.methods {
                            v.push(format!("{}.{}", cls.name.name, m.name.name));
                        }
                        v
                    }
                    _ => vec![],
                };
                for name in names {
                    func_to_file.insert(
                        name.to_uppercase(),
                        (path.clone(), content.clone()),
                    );
                }
            }
        }

        info!(
            "Source map built: {} files, {} functions, stdlib_len={stdlib_len}",
            file_virtual_offsets.len(),
            func_to_file.len(),
        );
        for (path, offset) in &file_virtual_offsets {
            let name = Path::new(path).file_name().unwrap_or_default().to_string_lossy();
            debug!("  {name}: virtual_offset={offset}");
        }

        Self { file_virtual_offsets, func_to_file }
    }

    /// Look up the virtual offset for a file path.
    fn virtual_offset(&self, path: &str) -> usize {
        self.file_virtual_offsets.get(path).copied().unwrap_or(0)
    }

    /// Find the source file (path, content) for a function name.
    fn file_for_func(&self, func_name: &str) -> Option<&(String, String)> {
        self.func_to_file.get(&func_name.to_uppercase())
    }
}

/// Handle a DAP attach session on a TCP stream.
///
/// Spawns a reader thread for TCP input and an event thread for engine
/// responses. The main loop dispatches between both without blocking.
pub fn handle_dap_attach(
    stream: std::net::TcpStream,
    app_state: Arc<AppState>,
    source_dir: &Path,
) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    info!("DAP attach: session from {peer}");

    // Attach to the running engine
    info!("DAP attach: calling debug_attach()...");
    let rt = tokio::runtime::Handle::current();
    let (cmd_tx, event_rx) = match rt.block_on(app_state.runtime_manager.debug_attach()) {
        Ok(channels) => {
            info!("DAP attach: debug_attach() succeeded — channels ready");
            channels
        }
        Err(e) => {
            error!("DAP attach: cannot attach to engine: {e}");
            send_dap_error(&stream, 0, &format!("Cannot attach: {e}"));
            return;
        }
    };

    // Load source files for breakpoint resolution
    let source_files = load_source_files(source_dir);

    // Path mapper for localRoot/remoteRoot translation (configured when
    // the attach request arrives with a localRoot argument).
    let mut path_mapper = PathMapper::new(source_dir);

    // Build source map for virtual offset resolution (same model as st-dap launch mode).
    // Uses discover_project to load files in the same sorted order as the compiler.
    let source_map = SourceMap::build(source_dir);

    // Create a unified input channel
    let (input_tx, input_rx) = std::sync::mpsc::channel::<Input>();

    // Spawn TCP reader thread
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            error!("DAP attach: cannot clone stream: {e}");
            return;
        }
    };
    let reader_tx = input_tx.clone();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_stream);
        loop {
            match read_dap_message(&mut reader) {
                Ok(Some(msg)) => {
                    if reader_tx.send(Input::DapMessage(msg)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    info!("DAP reader: EOF on TCP stream");
                    let _ = reader_tx.send(Input::DapDisconnected);
                    break;
                }
                Err(e) => {
                    warn!("DAP reader: error reading message: {e}");
                    let _ = reader_tx.send(Input::DapDisconnected);
                    break;
                }
            }
        }
    });

    // Spawn engine event thread
    let event_tx = input_tx;
    std::thread::spawn(move || {
        loop {
            match event_rx.recv() {
                Ok(resp) => {
                    if event_tx.send(Input::EngineEvent(resp)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    info!("Engine event thread: channel closed ({e})");
                    let _ = event_tx.send(Input::EngineDisconnected);
                    break;
                }
            }
        }
    });

    // Main event loop
    let writer = stream;
    let mut seq_counter: i64 = 1;
    let mut is_paused = false;
    let mut stop_on_entry = false;

    while let Ok(input) = input_rx.recv() {

        match input {
            Input::DapMessage(msg) => {
                handle_dap_request(
                    &msg,
                    &cmd_tx,
                    &writer,
                    &mut seq_counter,
                    &mut is_paused,
                    &source_files,
                    &source_map,
                    &input_rx,
                    &mut stop_on_entry,
                    &mut path_mapper,
                );
                let command = msg["command"].as_str().unwrap_or("");
                if command == "disconnect" {
                    break;
                }
            }
            Input::EngineEvent(resp) => {
                handle_engine_event(resp, &writer, &mut seq_counter, &mut is_paused);
            }
            Input::DapDisconnected => {
                info!("DAP attach: client disconnected");
                break;
            }
            Input::EngineDisconnected => {
                info!("DAP attach: engine disconnected");
                send_dap_event(&writer, &mut seq_counter, "terminated", serde_json::json!({}));
                break;
            }
        }
    }

    // Ensure clean detach
    if let Err(e) = cmd_tx.send(st_engine::DebugCommand::Disconnect) {
        debug!("DAP attach: final disconnect send failed (expected if already detached): {e}");
    }
    info!("DAP attach: session ended for {peer}");
}

// =============================================================================
// DAP request handling
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn handle_dap_request(
    msg: &serde_json::Value,
    cmd_tx: &std::sync::mpsc::Sender<st_engine::DebugCommand>,
    writer: &std::net::TcpStream,
    seq: &mut i64,
    is_paused: &mut bool,
    source_files: &[(String, String)],
    source_map: &SourceMap,
    input_rx: &std::sync::mpsc::Receiver<Input>,
    stop_on_entry: &mut bool,
    path_mapper: &mut PathMapper,
) {
    let req_seq = msg["seq"].as_i64().unwrap_or(0);
    let command = msg["command"].as_str().unwrap_or("");
    tracing::debug!("DAP attach: request seq={req_seq} command={command}");

    match command {
        "initialize" => {
            info!("DAP attach: initialize — sending capabilities");
            send_dap_response(writer, req_seq, "initialize", serde_json::json!({
                "supportsConfigurationDoneRequest": true,
                "supportsEvaluateForHovers": true,
            }));
        }

        "attach" | "launch" => {
            *stop_on_entry = msg["arguments"]["stopOnEntry"].as_bool().unwrap_or(false);
            if let Some(local_root) = msg["arguments"]["localRoot"].as_str() {
                path_mapper.set_local_root(local_root.to_string());
                info!(
                    "DAP attach: path mapping active — localRoot={local_root}, remoteRoot={}",
                    path_mapper.remote_root
                );
            }
            send_dap_response(writer, req_seq, command, serde_json::json!(null));
            send_dap_event(writer, seq, "initialized", serde_json::json!({}));
        }

        "configurationDone" => {
            info!("DAP attach: configurationDone (stopOnEntry={stop_on_entry})");
            send_dap_response(writer, req_seq, "configurationDone", serde_json::json!(null));
            if *stop_on_entry {
                info!("DAP attach: sending Pause for stopOnEntry");
                if let Err(e) = cmd_tx.send(st_engine::DebugCommand::Pause) {
                    error!("DAP attach: failed to send Pause: {e}");
                }
            }
        }

        "loadedSources" => {
            send_dap_response(writer, req_seq, "loadedSources", serde_json::json!({
                "sources": [],
            }));
        }

        "setExceptionBreakpoints" => {
            send_dap_response(writer, req_seq, "setExceptionBreakpoints", serde_json::json!(null));
        }

        "threads" => {
            send_dap_response(writer, req_seq, "threads", serde_json::json!({
                "threads": [{ "id": 1, "name": "PLC Scan Cycle" }]
            }));
        }

        "setBreakpoints" => {
            let client_path = msg["arguments"]["source"]["path"]
                .as_str().unwrap_or("").to_string();
            // Remap client-side path to target-side path for source lookup
            let source_path = path_mapper.to_remote(&client_path);
            let bp_lines: Vec<u32> = msg["arguments"]["breakpoints"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|b| b["line"].as_u64().map(|l| l as u32)).collect())
                .unwrap_or_default();

            let source_content = find_source_content(source_files, &source_path);
            let voff = source_map.virtual_offset(&source_path);

            info!(
                "setBreakpoints: {} → {} (voff={voff}, content_len={}, lines={:?})",
                client_path,
                Path::new(&source_path).file_name().unwrap_or_default().to_string_lossy(),
                source_content.len(),
                bp_lines,
            );
            if source_content.is_empty() {
                warn!(
                    "setBreakpoints: source content is EMPTY for {} — breakpoints will fail",
                    source_path,
                );
            }

            if let Err(e) = cmd_tx.send(st_engine::DebugCommand::SetBreakpoints {
                source_path,
                source: source_content,
                lines: bp_lines.clone(),
                source_offset: voff,
            }) {
                error!("setBreakpoints: failed to send to engine: {e}");
            }

            // The BreakpointsSet response will arrive as an EngineEvent.
            // For now, respond optimistically (all verified). The engine
            // will send the actual verification asynchronously.
            let breakpoints: Vec<serde_json::Value> = bp_lines.iter().enumerate()
                .map(|(i, line)| serde_json::json!({
                    "id": i + 1,
                    "verified": true,
                    "line": line,
                }))
                .collect();
            send_dap_response(writer, req_seq, "setBreakpoints", serde_json::json!({
                "breakpoints": breakpoints,
            }));
        }

        "continue" => {
            info!("DAP: continue (is_paused={is_paused})");
            send_dap_response(writer, req_seq, "continue", serde_json::json!({
                "allThreadsContinued": true,
            }));
            if *is_paused {
                if let Err(e) = cmd_tx.send(st_engine::DebugCommand::Continue) {
                    error!("DAP: failed to send Continue to engine: {e}");
                }
                *is_paused = false;
            } else {
                warn!("DAP: continue ignored — not paused");
            }
        }

        "next" => {
            info!("DAP: next/stepOver (is_paused={is_paused})");
            send_dap_response(writer, req_seq, "next", serde_json::json!(null));
            if *is_paused {
                if let Err(e) = cmd_tx.send(st_engine::DebugCommand::StepOver) {
                    error!("DAP: failed to send StepOver to engine: {e}");
                }
                *is_paused = false;
            } else {
                warn!("DAP: next ignored — not paused");
            }
        }

        "stepIn" => {
            info!("DAP: stepIn (is_paused={is_paused})");
            send_dap_response(writer, req_seq, "stepIn", serde_json::json!(null));
            if *is_paused {
                if let Err(e) = cmd_tx.send(st_engine::DebugCommand::StepIn) {
                    error!("DAP: failed to send StepIn to engine: {e}");
                }
                *is_paused = false;
            } else {
                warn!("DAP: stepIn ignored — not paused");
            }
        }

        "stepOut" => {
            info!("DAP: stepOut (is_paused={is_paused})");
            send_dap_response(writer, req_seq, "stepOut", serde_json::json!(null));
            if *is_paused {
                if let Err(e) = cmd_tx.send(st_engine::DebugCommand::StepOut) {
                    error!("DAP: failed to send StepOut to engine: {e}");
                }
                *is_paused = false;
            } else {
                warn!("DAP: stepOut ignored — not paused");
            }
        }

        "pause" => {
            info!("DAP: pause");
            if let Err(e) = cmd_tx.send(st_engine::DebugCommand::Pause) {
                error!("DAP: failed to send Pause: {e}");
            }
            send_dap_response(writer, req_seq, "pause", serde_json::json!(null));
            // Stopped event will arrive via EngineEvent
        }

        "stackTrace" => {
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::GetStackTrace);
                let frames = wait_for_engine_response(input_rx, seq, is_paused, writer,
                    |resp| matches!(resp, st_engine::DebugResponse::StackTrace { .. }),
                );
                if let Some(st_engine::DebugResponse::StackTrace { frames }) = frames {
                    let stack_frames: Vec<serde_json::Value> = frames.iter().enumerate()
                        .map(|(i, f)| {
                            let (line, spath) = resolve_frame_location(f, source_files, source_map);
                            let mapped = path_mapper.to_local(&spath);
                            serde_json::json!({
                                "id": i,
                                "name": f.func_name,
                                "source": { "name": std::path::Path::new(&spath).file_name().unwrap_or_default().to_string_lossy(), "path": mapped },
                                "line": line,
                                "column": 1,
                            })
                        })
                        .collect();
                    send_dap_response(writer, req_seq, "stackTrace", serde_json::json!({
                        "stackFrames": stack_frames,
                    }));
                } else {
                    send_dap_response(writer, req_seq, "stackTrace", serde_json::json!({
                        "stackFrames": [],
                    }));
                }
            } else {
                send_dap_response(writer, req_seq, "stackTrace", serde_json::json!({
                    "stackFrames": [],
                }));
            }
        }

        "scopes" => {
            send_dap_response(writer, req_seq, "scopes", serde_json::json!({
                "scopes": [
                    { "name": "Locals", "variablesReference": 1000, "presentationHint": "locals" },
                    { "name": "Globals", "variablesReference": 1001, "presentationHint": "registers" },
                ]
            }));
        }

        "variables" => {
            let var_ref = msg["arguments"]["variablesReference"].as_i64().unwrap_or(0);
            if *is_paused {
                let scope = if var_ref == 1001 {
                    st_engine::DebugScopeKind::Globals
                } else {
                    st_engine::DebugScopeKind::Locals
                };
                let _ = cmd_tx.send(st_engine::DebugCommand::GetVariables { scope });
                let resp = wait_for_engine_response(input_rx, seq, is_paused, writer,
                    |r| matches!(r, st_engine::DebugResponse::Variables { .. }),
                );
                if let Some(st_engine::DebugResponse::Variables { vars }) = resp {
                    let variables: Vec<serde_json::Value> = vars.iter()
                        .map(|v| serde_json::json!({
                            "name": v.name,
                            "value": v.value,
                            "type": v.ty,
                            "variablesReference": 0,
                        }))
                        .collect();
                    send_dap_response(writer, req_seq, "variables", serde_json::json!({
                        "variables": variables,
                    }));
                } else {
                    send_dap_response(writer, req_seq, "variables", serde_json::json!({
                        "variables": [],
                    }));
                }
            } else {
                send_dap_response(writer, req_seq, "variables", serde_json::json!({
                    "variables": [],
                }));
            }
        }

        "evaluate" => {
            let expr = msg["arguments"]["expression"].as_str().unwrap_or("");
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::Evaluate {
                    expression: expr.to_string(),
                });
                let resp = wait_for_engine_response(input_rx, seq, is_paused, writer,
                    |r| matches!(r, st_engine::DebugResponse::EvaluateResult { .. }),
                );
                if let Some(st_engine::DebugResponse::EvaluateResult { value, ty }) = resp {
                    send_dap_response(writer, req_seq, "evaluate", serde_json::json!({
                        "result": value,
                        "type": ty,
                        "variablesReference": 0,
                    }));
                } else {
                    send_dap_response(writer, req_seq, "evaluate", serde_json::json!({
                        "result": "<timeout>",
                        "variablesReference": 0,
                    }));
                }
            } else {
                send_dap_response(writer, req_seq, "evaluate", serde_json::json!({
                    "result": "<running>",
                    "variablesReference": 0,
                }));
            }
        }

        "disconnect" => {
            info!("DAP attach: disconnect — detaching from engine");
            if let Err(e) = cmd_tx.send(st_engine::DebugCommand::Disconnect) {
                debug!("DAP attach: disconnect send failed (may already be detached): {e}");
            }
            send_dap_response(writer, req_seq, "disconnect", serde_json::json!(null));
        }

        other => {
            warn!("DAP: unhandled command '{other}' — responding with empty success");
            send_dap_response(writer, req_seq, other, serde_json::json!(null));
        }
    }
}

// =============================================================================
// Engine event handling
// =============================================================================

fn handle_engine_event(
    resp: st_engine::DebugResponse,
    writer: &std::net::TcpStream,
    seq: &mut i64,
    is_paused: &mut bool,
) {
    match resp {
        st_engine::DebugResponse::Stopped { reason } => {
            *is_paused = true;
            let reason_str = match reason {
                st_engine::debug::PauseReason::Breakpoint => "breakpoint",
                st_engine::debug::PauseReason::Step => "step",
                st_engine::debug::PauseReason::PauseRequest => "pause",
                st_engine::debug::PauseReason::Entry => "entry",
                st_engine::debug::PauseReason::None => "pause",
            };
            info!("Engine event: Stopped (reason={reason_str})");
            send_dap_event(writer, seq, "stopped", serde_json::json!({
                "reason": reason_str,
                "threadId": 1,
                "allThreadsStopped": true,
            }));
        }
        st_engine::DebugResponse::Resumed => {
            info!("Engine event: Resumed");
            *is_paused = false;
        }
        st_engine::DebugResponse::Detached => {
            info!("Engine event: Detached");
            *is_paused = false;
            send_dap_event(writer, seq, "terminated", serde_json::json!({}));
        }
        st_engine::DebugResponse::Variables { vars } => {
            debug!("Engine event: Variables ({} entries)", vars.len());
            let _ = vars;
        }
        st_engine::DebugResponse::StackTrace { frames } => {
            debug!("Engine event: StackTrace ({} frames)", frames.len());
            let _ = frames;
        }
        st_engine::DebugResponse::EvaluateResult { value, ty } => {
            debug!("Engine event: EvaluateResult ({ty})");
            let _ = (value, ty);
        }
        st_engine::DebugResponse::BreakpointsSet { verified } => {
            let set_count = verified.iter().filter(|v| **v).count();
            info!(
                "Engine event: BreakpointsSet — {set_count}/{} verified",
                verified.len()
            );
        }
    }
}

// =============================================================================
// DAP message I/O helpers
// =============================================================================

fn read_dap_message(reader: &mut BufReader<std::net::TcpStream>) -> Result<Option<serde_json::Value>, String> {
    let mut header = String::new();
    loop {
        header.clear();
        match reader.read_line(&mut header) {
            Ok(0) => return Ok(None),
            Ok(_) => {}
            Err(e) => return Err(format!("Read error: {e}")),
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            let content_length: usize = len_str.trim().parse()
                .map_err(|_| "Invalid Content-Length".to_string())?;
            let mut blank = String::new();
            let _ = reader.read_line(&mut blank);
            let mut body = vec![0u8; content_length];
            std::io::Read::read_exact(reader, &mut body)
                .map_err(|e| format!("Body read error: {e}"))?;
            let json: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|e| format!("JSON parse error: {e}"))?;
            return Ok(Some(json));
        }
    }
    Ok(None)
}

fn send_dap_message(stream: &std::net::TcpStream, json: &serde_json::Value) {
    let body = serde_json::to_string(json).unwrap_or_default();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut s = stream;
    let _ = s.write_all(header.as_bytes());
    let _ = s.write_all(body.as_bytes());
    let _ = s.flush();
}

fn send_dap_response(stream: &std::net::TcpStream, req_seq: i64, command: &str, body: serde_json::Value) {
    send_dap_message(stream, &serde_json::json!({
        "seq": 0,
        "type": "response",
        "request_seq": req_seq,
        "success": true,
        "command": command,
        "body": body,
    }));
}

fn send_dap_event(stream: &std::net::TcpStream, seq: &mut i64, event: &str, body: serde_json::Value) {
    *seq += 1;
    send_dap_message(stream, &serde_json::json!({
        "seq": *seq,
        "type": "event",
        "event": event,
        "body": body,
    }));
}

/// Wait for a specific engine response, draining other events from the input
/// channel. Times out after 2 seconds. Any Stopped/Detached events that arrive
/// while waiting are forwarded to VS Code immediately.
fn wait_for_engine_response(
    input_rx: &std::sync::mpsc::Receiver<Input>,
    seq: &mut i64,
    is_paused: &mut bool,
    writer: &std::net::TcpStream,
    predicate: fn(&st_engine::DebugResponse) -> bool,
) -> Option<st_engine::DebugResponse> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match input_rx.recv_timeout(remaining) {
            Ok(Input::EngineEvent(resp)) => {
                if predicate(&resp) {
                    return Some(resp);
                }
                // Forward other engine events (e.g., Stopped) while waiting
                handle_engine_event(resp, writer, seq, is_paused);
            }
            Ok(Input::DapMessage(_)) => {
                // Ignore DAP messages while waiting for engine response
                // (they'll be processed on the next main loop iteration)
            }
            Ok(Input::DapDisconnected) | Ok(Input::EngineDisconnected) => {
                return None;
            }
            Err(_) => {
                warn!("wait_for_engine_response: timed out after 2s");
                return None;
            }
        }
    }
}

/// Resolve a stack frame's source offset to a line number and file path.
///
/// `frame.source_offset` is a virtual offset (byte position in the concatenated
/// stdlib + project sources). We subtract the file's virtual base offset to get
/// a file-local byte position, then count newlines to get the line number.
fn resolve_frame_location(
    frame: &st_engine::debug::FrameInfo,
    source_files: &[(String, String)],
    source_map: &SourceMap,
) -> (u32, String) {
    // Use func_to_file mapping (same approach as st-dap launch mode)
    if let Some((path, content)) = source_map.file_for_func(&frame.func_name) {
        let voff = source_map.virtual_offset(path);
        let local_offset = frame.source_offset.saturating_sub(voff).min(content.len());
        let line = byte_offset_to_line(content, local_offset);
        debug!(
            "stackTrace: {} → {}, voff={voff}, local_offset={local_offset}, line={line}",
            frame.func_name,
            Path::new(path).file_name().unwrap_or_default().to_string_lossy(),
        );
        return (line, path.clone());
    }

    // Fallback: try first source file
    if let Some((path, content)) = source_files.first() {
        let voff = source_map.virtual_offset(path);
        let local_offset = frame.source_offset.saturating_sub(voff).min(content.len());
        let line = byte_offset_to_line(content, local_offset);
        warn!(
            "stackTrace: {} not in func_to_file map, fell back to {}",
            frame.func_name,
            Path::new(path).file_name().unwrap_or_default().to_string_lossy(),
        );
        return (line, path.clone());
    }

    (1, String::new())
}

/// Convert a byte offset within source text to a 1-based line number.
fn byte_offset_to_line(source: &str, offset: usize) -> u32 {
    let offset = offset.min(source.len());
    source[..offset].matches('\n').count() as u32 + 1
}

fn send_dap_error(stream: &std::net::TcpStream, req_seq: i64, message: &str) {
    send_dap_message(stream, &serde_json::json!({
        "seq": 0,
        "type": "response",
        "request_seq": req_seq,
        "success": false,
        "message": message,
    }));
}

// =============================================================================
// Source file helpers
// =============================================================================

fn load_source_files(source_dir: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    if source_dir.is_dir() {
        collect_st_files(source_dir, &mut files);
    }
    files
}

fn collect_st_files(dir: &Path, files: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_st_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "st" || e == "scl") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                files.push((path.to_string_lossy().to_string(), content));
            }
        }
    }
}

fn find_source_content(source_files: &[(String, String)], path: &str) -> String {
    if let Some((_, content)) = source_files.iter().find(|(p, _)| p == path) {
        return content.clone();
    }
    let basename = Path::new(path).file_name().unwrap_or_default().to_string_lossy();
    if let Some((_, content)) = source_files.iter().find(|(p, _)| {
        Path::new(p).file_name().unwrap_or_default().to_string_lossy() == basename
    }) {
        return content.clone();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mapper_to_local_basic() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        assert_eq!(
            m.to_local("/var/lib/st-plc/programs/current_source/main.st"),
            "/home/user/plc-project/main.st"
        );
    }

    #[test]
    fn path_mapper_to_local_subdirectory() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        assert_eq!(
            m.to_local("/var/lib/st-plc/programs/current_source/controllers/fill.st"),
            "/home/user/plc-project/controllers/fill.st"
        );
    }

    #[test]
    fn path_mapper_to_remote_basic() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        assert_eq!(
            m.to_remote("/home/user/plc-project/main.st"),
            "/var/lib/st-plc/programs/current_source/main.st"
        );
    }

    #[test]
    fn path_mapper_to_remote_subdirectory() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        assert_eq!(
            m.to_remote("/home/user/plc-project/controllers/fill.st"),
            "/var/lib/st-plc/programs/current_source/controllers/fill.st"
        );
    }

    #[test]
    fn path_mapper_windows_separators() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("C:/Users/dev/project".into());
        assert_eq!(
            m.to_remote("C:\\Users\\dev\\project\\controllers\\fill.st"),
            "/var/lib/st-plc/programs/current_source/controllers/fill.st"
        );
    }

    #[test]
    fn path_mapper_no_local_root_passthrough() {
        let m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        assert_eq!(
            m.to_local("/var/lib/st-plc/programs/current_source/main.st"),
            "/var/lib/st-plc/programs/current_source/main.st"
        );
        assert_eq!(m.to_remote("/some/path/main.st"), "/some/path/main.st");
    }

    #[test]
    fn path_mapper_roundtrip() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        let remote = "/var/lib/st-plc/programs/current_source/controllers/fill.st";
        let local = m.to_local(remote);
        assert_eq!(m.to_remote(&local), remote);
    }

    #[test]
    fn path_mapper_trailing_slash() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source/"));
        m.set_local_root("/home/user/plc-project/".into());
        assert_eq!(
            m.to_local("/var/lib/st-plc/programs/current_source/main.st"),
            "/home/user/plc-project/main.st"
        );
        assert_eq!(
            m.to_remote("/home/user/plc-project/main.st"),
            "/var/lib/st-plc/programs/current_source/main.st"
        );
    }

    #[test]
    fn path_mapper_unrelated_path_passthrough() {
        let mut m = PathMapper::new(Path::new("/var/lib/st-plc/programs/current_source"));
        m.set_local_root("/home/user/plc-project".into());
        assert_eq!(m.to_local("/some/other/file.st"), "/some/other/file.st");
        assert_eq!(m.to_remote("/some/other/file.st"), "/some/other/file.st");
    }
}
