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
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

/// Messages from the reader thread to the main loop.
enum Input {
    DapMessage(serde_json::Value),
    DapDisconnected,
    EngineEvent(st_engine::DebugResponse),
    EngineDisconnected,
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
    let rt = tokio::runtime::Handle::current();
    let (cmd_tx, event_rx) = match rt.block_on(app_state.runtime_manager.debug_attach()) {
        Ok(channels) => channels,
        Err(e) => {
            error!("DAP attach: cannot attach to engine: {e}");
            send_dap_error(&stream, 0, &format!("Cannot attach: {e}"));
            return;
        }
    };

    // Load source files for breakpoint resolution
    let source_files = load_source_files(source_dir);

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
                _ => {
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
                Err(_) => {
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
                    &input_rx,
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
    let _ = cmd_tx.send(st_engine::DebugCommand::Disconnect);
    info!("DAP attach: session ended for {peer}");
}

// =============================================================================
// DAP request handling
// =============================================================================

fn handle_dap_request(
    msg: &serde_json::Value,
    cmd_tx: &std::sync::mpsc::Sender<st_engine::DebugCommand>,
    writer: &std::net::TcpStream,
    seq: &mut i64,
    is_paused: &mut bool,
    source_files: &[(String, String)],
    input_rx: &std::sync::mpsc::Receiver<Input>,
) {
    let req_seq = msg["seq"].as_i64().unwrap_or(0);
    let command = msg["command"].as_str().unwrap_or("");

    match command {
        "initialize" => {
            send_dap_response(writer, req_seq, "initialize", serde_json::json!({
                "supportsConfigurationDoneRequest": true,
                "supportsEvaluateForHovers": true,
            }));
        }

        "attach" => {
            send_dap_response(writer, req_seq, "attach", serde_json::json!(null));
            // Send Initialized event so VS Code sends configurationDone
            send_dap_event(writer, seq, "initialized", serde_json::json!({}));
            // Do NOT pause or send Stopped — the engine keeps running.
            // VS Code shows the debug toolbar with a Pause button.
        }

        "configurationDone" => {
            send_dap_response(writer, req_seq, "configurationDone", serde_json::json!(null));
        }

        "threads" => {
            send_dap_response(writer, req_seq, "threads", serde_json::json!({
                "threads": [{ "id": 1, "name": "PLC Scan Cycle" }]
            }));
        }

        "setBreakpoints" => {
            let source_path = msg["arguments"]["source"]["path"]
                .as_str().unwrap_or("").to_string();
            let bp_lines: Vec<u32> = msg["arguments"]["breakpoints"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|b| b["line"].as_u64().map(|l| l as u32)).collect())
                .unwrap_or_default();

            let source_content = find_source_content(source_files, &source_path);

            let _ = cmd_tx.send(st_engine::DebugCommand::SetBreakpoints {
                source_path,
                source: source_content,
                lines: bp_lines.clone(),
            });

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
            send_dap_response(writer, req_seq, "continue", serde_json::json!({
                "allThreadsContinued": true,
            }));
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::Continue);
                *is_paused = false;
            }
            // Engine is now running — Stopped event will arrive via EngineEvent
        }

        "next" => {
            send_dap_response(writer, req_seq, "next", serde_json::json!(null));
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::StepOver);
                *is_paused = false;
            }
        }

        "stepIn" => {
            send_dap_response(writer, req_seq, "stepIn", serde_json::json!(null));
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::StepIn);
                *is_paused = false;
            }
        }

        "stepOut" => {
            send_dap_response(writer, req_seq, "stepOut", serde_json::json!(null));
            if *is_paused {
                let _ = cmd_tx.send(st_engine::DebugCommand::StepOut);
                *is_paused = false;
            }
        }

        "pause" => {
            let _ = cmd_tx.send(st_engine::DebugCommand::Pause);
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
                            let (line, spath) = resolve_frame_location(f, source_files);
                            serde_json::json!({
                                "id": i,
                                "name": f.func_name,
                                "source": { "name": std::path::Path::new(&spath).file_name().unwrap_or_default().to_string_lossy(), "path": spath },
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
            let _ = cmd_tx.send(st_engine::DebugCommand::Disconnect);
            send_dap_response(writer, req_seq, "disconnect", serde_json::json!(null));
        }

        other => {
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
            send_dap_event(writer, seq, "stopped", serde_json::json!({
                "reason": reason_str,
                "threadId": 1,
                "allThreadsStopped": true,
            }));
        }
        st_engine::DebugResponse::Resumed => {
            *is_paused = false;
        }
        st_engine::DebugResponse::Detached => {
            *is_paused = false;
            send_dap_event(writer, seq, "terminated", serde_json::json!({}));
        }
        st_engine::DebugResponse::Variables { vars } => {
            // Late-arriving variable response — VS Code already got an
            // empty response. This is a known limitation of the async
            // architecture. Phase E will add proper request/response
            // correlation with sequence numbers.
            let _ = vars; // TODO: improve with request correlation
        }
        st_engine::DebugResponse::StackTrace { frames } => {
            let _ = frames; // TODO: improve with request correlation
        }
        st_engine::DebugResponse::EvaluateResult { value, ty } => {
            let _ = (value, ty); // TODO: improve with request correlation
        }
        st_engine::DebugResponse::BreakpointsSet { verified } => {
            let _ = verified; // Already responded optimistically
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
            Err(_) => return None, // Timeout
        }
    }
}

/// Resolve a stack frame's source offset to a line number and file path.
fn resolve_frame_location(
    frame: &st_engine::debug::FrameInfo,
    source_files: &[(String, String)],
) -> (u32, String) {
    for (path, content) in source_files {
        let upper = content.to_uppercase();
        for keyword in ["PROGRAM ", "FUNCTION_BLOCK ", "FUNCTION ", "CLASS "] {
            if let Some(idx) = upper.find(keyword) {
                let after = idx + keyword.len();
                let name_end = content[after..].find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(content.len() - after);
                let name = &content[after..after + name_end];
                if name.eq_ignore_ascii_case(&frame.func_name)
                    || frame.func_name.to_uppercase().starts_with(&name.to_uppercase())
                {
                    let offset = frame.source_offset.min(content.len());
                    let line = content[..offset].matches('\n').count() as u32 + 1;
                    return (line, path.clone());
                }
            }
        }
    }
    if let Some((path, content)) = source_files.first() {
        let offset = frame.source_offset.min(content.len());
        let line = content[..offset].matches('\n').count() as u32 + 1;
        return (line, path.clone());
    }
    (1, String::new())
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
