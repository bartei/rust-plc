//! DAP server implementation.

use dap::events::*;
use dap::prelude::*;
use dap::requests::Command;
use dap::responses::{ResponseBody, ResponseMessage};
use dap::types::*;
use st_comm_api::CommDevice;
use st_ir::PouKind;
use dap::base_message::Sendable;
use st_engine::comm_manager::CommManager;
use st_engine::debug::{PauseReason, StepMode};
use st_engine::engine::CycleStats;
use st_engine::vm::{Vm, VmConfig, VmError};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::comm_setup;

/// Read one DAP-framed message (`Content-Length: N\r\n\r\n<json>`) from the
/// given buffered reader and parse it as a `Request`. Returns `Ok(None)` on
/// EOF. Used both by the production reader thread and the in-process tests.
fn read_dap_request<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Request>> {
    let mut content_length: usize = 0;
    let mut line = String::new();

    // Header section: read lines until we hit the empty separator line.
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Bad Content-Length: {e}"),
                )
            })?;
        }
        // Other headers are ignored — we only care about Content-Length.
    }

    if content_length == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Missing or zero Content-Length",
        ));
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let req: Request = serde_json::from_slice(&body).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid DAP JSON: {e}"),
        )
    })?;
    Ok(Some(req))
}

/// DAP message writer. Owns the output buffer + sequence counter. Mirrors
/// the wire format of `dap::ServerOutput::send` so VS Code accepts it.
struct DapWriter<W: Write> {
    out: BufWriter<W>,
    seq: i64,
}

impl<W: Write> DapWriter<W> {
    fn new(output: W) -> Self {
        Self {
            out: BufWriter::new(output),
            seq: 0,
        }
    }

    fn send(&mut self, body: Sendable) -> std::io::Result<()> {
        self.seq += 1;
        let message = dap::base_message::BaseMessage {
            seq: self.seq,
            message: body,
        };
        let json = serde_json::to_string(&message).map_err(|e| {
            std::io::Error::other(format!("Serialize failed: {e}"))
        })?;
        write!(self.out, "Content-Length: {}\r\n\r\n", json.len())?;
        write!(self.out, "{json}\r\n")?;
        self.out.flush()
    }

    fn respond(&mut self, response: Response) -> std::io::Result<()> {
        self.send(Sendable::Response(response))
    }

    fn send_event(&mut self, event: Event) -> std::io::Result<()> {
        self.send(Sendable::Event(event))
    }
}

/// Run the DAP server on the given reader/writer.
///
/// Architecture: a dedicated reader thread owns the input stream and pushes
/// parsed `Request`s onto an mpsc channel. The main thread owns the writer
/// and the `DapSession`, blocks on `recv()` for the next request, and after
/// each `handle_request` call drains any deferred requests/responses that
/// the (interruptible) run loop accumulated.
pub fn run_dap<R, W>(input: R, output: W, source_path: &str)
where
    R: Read + Send + 'static,
    W: Write,
{
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let mut writer = DapWriter::new(output);
    let mut session = DapSession::new(source_path);
    session.set_request_rx(req_rx);

    eprintln!("[DAP] Server started for: {source_path}");

    // Spawn the reader thread. It owns the input stream, parses framed DAP
    // messages, and pushes them onto the channel until EOF or a parse error.
    let _reader_handle = thread::spawn(move || {
        let mut reader = BufReader::new(input);
        loop {
            match read_dap_request(&mut reader) {
                Ok(Some(req)) => {
                    if req_tx.send(req).is_err() {
                        // Receiver dropped — main loop has exited.
                        break;
                    }
                }
                Ok(None) => {
                    eprintln!("[DAP] Reader thread: EOF");
                    break;
                }
                Err(e) => {
                    eprintln!("[DAP] Reader thread: read error: {e}");
                    break;
                }
            }
        }
    });

    // Main loop: pull requests from the channel, handle them, send responses
    // and any pending events. The session may have queued additional
    // requests/responses while running cycles inside resume_execution; drain
    // those after each top-level handle_request returns.
    loop {
        // Block on next request. Scoped so the immutable borrow ends before
        // we call session.handle_request below.
        let req = {
            let rx = match session.request_rx.as_ref() {
                Some(rx) => rx,
                None => {
                    eprintln!("[DAP] No request receiver");
                    break;
                }
            };
            match rx.recv() {
                Ok(r) => r,
                Err(_) => {
                    eprintln!("[DAP] Request channel closed");
                    break;
                }
            }
        };

        eprintln!("[DAP] Request: {:?}", req.command);
        let is_resume = DapSession::is_resume_command(&req.command);

        // For resume commands (Continue, Step, etc.) we MUST send the
        // response BEFORE the blocking run loop starts, so VS Code can
        // transition to "running" state immediately — flipping the
        // play/pause button and clearing the yellow highlight.
        let response = session.handle_request(&req);
        eprintln!("[DAP] Response: success={}", response.success);
        if writer.respond(response).is_err() {
            eprintln!("[DAP] Failed to send response");
            break;
        }

        // Flush any events generated during handle_request (Stopped on
        // entry, Initialized, console output, etc.) BEFORE the run loop.
        for event in session.pending_events.drain(..) {
            eprintln!("[DAP] Event: {event:?}");
            if writer.send_event(event).is_err() {
                eprintln!("[DAP] Failed to send event");
                break;
            }
        }

        if is_resume {
            // Now enter the blocking run loop. The response is already
            // sent, so VS Code is in "running" state. resume_execution
            // streams telemetry events to the writer in real time; the
            // Stopped / Terminated event left over in pending_events is
            // flushed here after it returns.
            session.handle_resume(&req.command, &mut writer);

            // Send events generated at loop exit (Stopped, Terminated,
            // final telemetry force-flush).
            for event in session.pending_events.drain(..) {
                eprintln!("[DAP] Event: {event:?}");
                if writer.send_event(event).is_err() {
                    eprintln!("[DAP] Failed to send event");
                    break;
                }
            }
        }

        // Drain responses generated inline during a run loop (e.g.,
        // SetBreakpoints applied while running).
        for resp in session.deferred_responses.drain(..) {
            if writer.respond(resp).is_err() {
                eprintln!("[DAP] Failed to send deferred response");
                break;
            }
        }

        // Drain requests received during a run loop. Process each one as if
        // it had arrived through the normal main-loop flow — this generates
        // their proper responses (and any follow-on events).
        let deferred: Vec<Request> = session.deferred_requests.drain(..).collect();
        for d in deferred {
            eprintln!("[DAP] Deferred request: {:?}", d.command);
            let resp = session.handle_request(&d);
            if writer.respond(resp).is_err() {
                eprintln!("[DAP] Failed to send deferred response");
                break;
            }
            for event in session.pending_events.drain(..) {
                if writer.send_event(event).is_err() {
                    break;
                }
            }
            // Inflight handlers may also have queued more responses
            for resp in session.deferred_responses.drain(..) {
                let _ = writer.respond(resp);
            }
        }

        if session.should_exit {
            eprintln!("[DAP] Session exit requested");
            break;
        }
    }
    // Make sure stdout is flushed before we drop everything.
    drop(writer);
}

fn ok(seq: i64, body: ResponseBody) -> Response {
    Response {
        request_seq: seq,
        success: true,
        message: None,
        body: Some(body),
        error: None,
    }
}

fn err(seq: i64, msg: &str) -> Response {
    Response {
        request_seq: seq,
        success: false,
        message: Some(ResponseMessage::Error(msg.to_string())),
        body: None,
        error: None,
    }
}

/// Helper to create a console output event (shows in VSCode Debug Console)
fn console_output(msg: &str) -> Event {
    Event::Output(OutputEventBody {
        category: Some(OutputEventCategory::Console),
        output: format!("{msg}\n"),
        ..Default::default()
    })
}

/// Build a `plc/cycleStats` telemetry event. We piggy-back on DAP's standard
/// `output` event with `category: telemetry` so we don't need to patch the
/// `dap` crate; the VS Code extension picks the payload up via a
/// `DebugAdapterTracker`. The `output` field carries a stable sentinel string
/// the tracker matches against, and `data` carries the structured payload.
fn cycle_stats_event(
    stats: &CycleStats,
    instructions_per_cycle: u64,
    devices_ok: u32,
    devices_err: u32,
    watchdog_us: Option<u64>,
    target_cycle_time: Option<Duration>,
    variables: &[serde_json::Value],
) -> Event {
    let to_us = |d: Duration| d.as_micros() as u64;
    let min_us = if stats.min_cycle_time == Duration::MAX {
        0
    } else {
        to_us(stats.min_cycle_time)
    };
    let min_period_us = if stats.min_cycle_period == Duration::MAX {
        0
    } else {
        to_us(stats.min_cycle_period)
    };
    let payload = serde_json::json!({
        "schema": 3,
        "cycle_count": stats.cycle_count,
        "last_us": to_us(stats.last_cycle_time),
        "min_us": min_us,
        "max_us": to_us(stats.max_cycle_time),
        "avg_us": to_us(stats.avg_cycle_time()),
        "instructions_per_cycle": instructions_per_cycle,
        "watchdog_us": watchdog_us,
        "devices_ok": devices_ok,
        "devices_err": devices_err,
        "target_us": target_cycle_time.map(to_us),
        "last_period_us": to_us(stats.last_cycle_period),
        "min_period_us": min_period_us,
        "max_period_us": to_us(stats.max_cycle_period),
        "jitter_max_us": to_us(stats.jitter_max),
        "variables": variables,
    });
    Event::Output(OutputEventBody {
        category: Some(OutputEventCategory::Telemetry),
        output: "plc/cycleStats".to_string(),
        data: Some(payload),
        ..Default::default()
    })
}

#[derive(Debug, Clone, Copy)]
enum ScopeKind {
    Locals,
    Globals,
}

/// Reference to a FB instance for hierarchical variable expansion.
/// When VS Code expands a FB in the Variables panel, we use this to
/// fetch the instance's field values from `vm.fb_instances`.
#[derive(Debug, Clone, Copy)]
struct FbVarRef {
    /// The caller identity (encodes who owns this FB instance).
    caller_id: u32,
    /// The slot index of this FB instance in its parent's locals.
    slot_idx: u16,
    /// The function index of the FB's definition (for locals layout).
    fb_func_idx: u16,
}

/// Reference to a struct instance for hierarchical variable expansion.
#[derive(Debug, Clone, Copy)]
struct StructVarRef {
    /// The caller identity (encodes who owns this struct instance).
    caller_id: u32,
    /// The slot index of this struct variable in its parent's locals.
    slot_idx: u16,
    /// The type_def index for this struct's field layout.
    type_def_idx: u16,
}

struct DapSession {
    source_path: String,
    source: String,
    vm: Option<Vm>,
    pending_events: Vec<Event>,
    should_exit: bool,
    next_var_ref: i64,
    entry_point_override: Option<String>,
    /// Maps function name → (source file path, source content) for multi-file debugging.
    func_source_map: std::collections::HashMap<String, (String, String)>,
    /// Source files loaded from the project: (path, content).
    project_files: Vec<(String, String)>,
    /// Accumulated breakpoints per file: path → (source_content, line_numbers).
    pending_breakpoints: std::collections::HashMap<String, (String, Vec<u32>)>,
    /// Maps variable reference IDs to scope kinds for Variables requests.
    scope_refs: std::collections::HashMap<i64, ScopeKind>,
    /// Maps variable reference IDs to FB instance locations for hierarchical
    /// expansion. When VS Code expands a FB instance in the Variables panel,
    /// it sends a Variables request with the ref ID → we look up the FB
    /// instance state and return its fields as children.
    fb_var_refs: std::collections::HashMap<i64, FbVarRef>,
    /// Maps variable reference IDs to struct instance locations for
    /// hierarchical expansion (same pattern as fb_var_refs).
    struct_var_refs: std::collections::HashMap<i64, StructVarRef>,
    /// Communication manager for simulated devices (read inputs / write outputs each scan).
    comm: CommManager,
    /// Per-file byte offsets in the virtual concatenated text produced by
    /// `parse_multi()`. Maps file path → virtual offset. Used by
    /// `set_line_breakpoints` to align file-local line numbers with the
    /// compiled module's source_map entries. For single-file mode, the
    /// only entry is `(self.source_path, stdlib_total_length)`.
    file_virtual_offsets: std::collections::HashMap<String, usize>,
    /// Comm setup data (config, profiles, generated source) loaded from plc-project.yaml.
    comm_setup: Option<comm_setup::CommSetup>,
    /// Cached native FB registry for compilation and VM creation.
    native_fb_registry: Option<std::sync::Arc<st_comm_api::NativeFbRegistry>>,
    /// Live scan cycle statistics (populated by the DAP run loop).
    cycle_stats: CycleStats,
    /// Instant the *currently in-flight* scan cycle started, or `None` between
    /// cycles. A single logical cycle may span multiple `vm.run` /
    /// `vm.continue_execution` calls when breakpoints are hit, so this is
    /// only set when a fresh cycle begins and only cleared when it completes.
    current_cycle_start: Option<Instant>,
    /// Instructions executed during the current in-flight cycle (kept across
    /// breakpoint pauses; reset when a new cycle starts).
    current_cycle_instructions: u64,
    /// Cycles executed since the last `plc/cycleStats` telemetry event.
    cycles_since_last_event: u32,
    /// Emit a telemetry event every N cycles. 0 disables periodic emission.
    cycle_event_interval: u32,
    /// Watchdog budget in microseconds, surfaced in telemetry payloads. Future
    /// versions will read this from `plc-project.yaml`.
    watchdog_us: Option<u64>,
    /// Tracks when the previous scan cycle started (for period / jitter
    /// calculation). Reset to `None` when the run loop exits via Halt so
    /// user-pause time doesn't pollute the measurement.
    previous_cycle_start: Option<Instant>,
    /// User's watch list — only these variables are snapshot into the
    /// telemetry payload's `variables` array. Empty list means "send no
    /// variables" (the user hasn't picked any to watch yet). Sending all
    /// variables every 500ms doesn't scale to projects with hundreds of
    /// I/O points, so the panel must opt-in.
    watched_variables: Vec<String>,
    /// Target scan cycle period from `engine.cycle_time` in plc-project.yaml.
    /// `None` means "run as fast as possible". When set, the DAP run loop
    /// sleeps `target - elapsed` between cycles in interruptible chunks.
    target_cycle_time: Option<Duration>,
    /// Receiver half of the request channel. Set by `run_dap` after the
    /// reader thread is spawned. The DAP run loop polls this between cycles
    /// to remain interruptible by `Pause`, `Disconnect`, `SetBreakpoints`,
    /// and other client requests during a long Continue.
    request_rx: Option<mpsc::Receiver<Request>>,
    /// Requests received during a run loop. Drained and processed by the
    /// outer `run_dap` loop after `resume_execution` returns.
    deferred_requests: Vec<Request>,
    /// Responses generated inline during a run loop (e.g., for SetBreakpoints
    /// applied while the program is executing). Sent by `run_dap` after
    /// `resume_execution` returns.
    deferred_responses: Vec<Response>,
    /// Monitor handle for the embedded WS server (WebSocket-based variable
    /// monitoring). Created during handle_launch.
    monitor_handle: Option<st_monitor::MonitorHandle>,
}

impl DapSession {
    fn new(source_path: &str) -> Self {
        Self {
            source_path: source_path.to_string(),
            source: String::new(),
            vm: None,
            pending_events: Vec::new(),
            should_exit: false,
            entry_point_override: None,
            func_source_map: std::collections::HashMap::new(),
            project_files: Vec::new(),
            pending_breakpoints: std::collections::HashMap::new(),
            scope_refs: std::collections::HashMap::new(),
            fb_var_refs: std::collections::HashMap::new(),
            struct_var_refs: std::collections::HashMap::new(),
            next_var_ref: 1000,
            comm: CommManager::new(),
            file_virtual_offsets: std::collections::HashMap::new(),
            comm_setup: None,
            native_fb_registry: None,
            cycle_stats: CycleStats {
                min_cycle_time: Duration::MAX,
                min_cycle_period: Duration::MAX,
                ..Default::default()
            },
            current_cycle_start: None,
            current_cycle_instructions: 0,
            cycles_since_last_event: 0,
            cycle_event_interval: 20,
            watchdog_us: None,
            previous_cycle_start: None,
            watched_variables: Vec::new(),
            target_cycle_time: None,
            request_rx: None,
            deferred_requests: Vec::new(),
            deferred_responses: Vec::new(),
            monitor_handle: None,
        }
    }

    /// Build a compact summary string for a FB instance, e.g., "CV=2, Q=FALSE".
    fn fb_summary_value(
        &self,
        vm: &st_engine::vm::Vm,
        caller_id: u32,
        slot_idx: u16,
        fb_func_idx: u16,
    ) -> String {
        let fb_func = &vm.module().functions[fb_func_idx as usize];
        let instance_key = (caller_id, slot_idx);
        let fb_state = vm.fb_instances_ref().get(&instance_key);
        let mut parts = Vec::new();
        // Show VAR_OUTPUT fields first (most interesting), then VAR_INPUT
        for (j, fb_slot) in fb_func.locals.slots.iter().enumerate() {
            if matches!(fb_slot.ty, st_ir::VarType::FbInstance(_) | st_ir::VarType::ClassInstance(_)) {
                continue; // skip nested FB/class instances in the summary
            }
            let val = fb_state
                .and_then(|s| s.get(j))
                .cloned()
                .unwrap_or(st_ir::Value::Void);
            if val == st_ir::Value::Void {
                continue;
            }
            parts.push(format!("{}={}", fb_slot.name, st_engine::debug::format_value(&val)));
            if parts.len() >= 4 {
                parts.push("...".to_string());
                break;
            }
        }
        if parts.is_empty() {
            "(no state)".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Build a compact summary string for a struct instance, e.g.,
    /// "bottles_filled=3, running=TRUE".
    fn struct_summary_value(
        &self,
        vm: &st_engine::vm::Vm,
        caller_id: u32,
        slot_idx: u16,
        type_def_idx: u16,
    ) -> String {
        let Some((_, fields)) = vm.struct_type_fields(type_def_idx) else {
            return "(no fields)".to_string();
        };
        let instance_key = (caller_id, slot_idx);
        let state = vm.fb_instances_ref().get(&instance_key);
        let mut parts = Vec::new();
        for (j, field) in fields.iter().enumerate() {
            let val = state
                .and_then(|s| s.get(j))
                .cloned()
                .unwrap_or(st_ir::Value::default_for_type(field.ty));
            if val == st_ir::Value::Void {
                continue;
            }
            parts.push(format!("{}={}", field.name, st_engine::debug::format_value(&val)));
            if parts.len() >= 4 {
                parts.push("...".to_string());
                break;
            }
        }
        if parts.is_empty() {
            "(no state)".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Look up the virtual offset for a file path. Falls back to:
    /// 1. Exact match in `file_virtual_offsets`
    /// 2. Canonical-path match
    /// 3. The primary source file's offset (single-file default)
    /// 4. Zero
    fn resolve_virtual_offset(&self, path: Option<&str>) -> usize {
        if let Some(p) = path {
            if let Some(v) = self.file_virtual_offsets.get(p) {
                return *v;
            }
            if let Ok(canon) = std::fs::canonicalize(p) {
                for (k, v) in &self.file_virtual_offsets {
                    if std::fs::canonicalize(k).ok().as_ref() == Some(&canon) {
                        return *v;
                    }
                }
            }
        }
        // Fall back to the primary source file's offset (for single-file
        // tests where VS Code sends a synthetic path like "test.st").
        self.file_virtual_offsets
            .get(&self.source_path)
            .copied()
            .unwrap_or(0)
    }

    /// Install the request channel receiver. Called by `run_dap` after the
    /// reader thread is spawned. Without a receiver the run loop falls back
    /// to non-interactive mode (legacy test path).
    pub fn set_request_rx(&mut self, rx: mpsc::Receiver<Request>) {
        self.request_rx = Some(rx);
    }

    /// Returns true if the command is a "resume" command (Continue, Next,
    /// StepIn, StepOut) whose response must be sent BEFORE the blocking
    /// run loop starts. This lets VS Code transition to "running" state
    /// immediately so the play/pause button flips and the yellow highlight
    /// is removed.
    fn is_resume_command(cmd: &Command) -> bool {
        matches!(
            cmd,
            Command::Continue(_)
                | Command::Next(_)
                | Command::StepIn(_)
                | Command::StepOut(_)
        )
    }

    /// Execute the resume action for a Continue/Step request. Called by the
    /// main loop AFTER the response has already been sent to VS Code.
    /// The `writer` is passed through so `resume_execution` can stream
    /// telemetry events to the wire in real time (every N cycles) rather
    /// than buffering them until the run loop exits.
    fn handle_resume<W: Write>(&mut self, cmd: &Command, writer: &mut DapWriter<W>) {
        let mode = match cmd {
            Command::Continue(_) => StepMode::Continue,
            Command::Next(_) => StepMode::StepOver,
            Command::StepIn(_) => StepMode::StepIn,
            Command::StepOut(_) => StepMode::StepOut,
            _ => return,
        };
        self.resume_execution(mode, writer);
    }

    fn handle_request(&mut self, req: &Request) -> Response {
        let seq = req.seq;
        match &req.command {
            Command::Initialize(_) => {
                // NOTE: Do NOT emit Initialized here. Emit it after Launch
                // so VS Code sends SetBreakpoints AFTER the VM exists.
                ok(seq, ResponseBody::Initialize(Capabilities {
                    supports_configuration_done_request: Some(true),
                    supports_evaluate_for_hovers: Some(true),
                    ..Default::default()
                }))
            }
            Command::Launch(_) => {
                let response = self.handle_launch(seq);
                // Emit Initialized AFTER launch so breakpoints are set against a live VM
                self.pending_events.push(Event::Initialized);
                response
            }
            Command::Attach(_) => {
                // Attach works the same as Launch — the proxy already set up the source.
                // We use the same load/compile logic but return an Attach response body.
                let mut response = self.handle_launch(seq);
                // Override the response body to match the Attach command
                if response.success {
                    response.body = Some(ResponseBody::Attach);
                }
                self.pending_events.push(Event::Initialized);
                response
            }
            Command::SetBreakpoints(args) => self.handle_set_breakpoints(seq, args),
            Command::ConfigurationDone => ok(seq, ResponseBody::ConfigurationDone),
            Command::Threads => ok(seq, ResponseBody::Threads(dap::responses::ThreadsResponse {
                threads: vec![Thread { id: 1, name: "PLC Scan Cycle".into() }],
            })),
            Command::StackTrace(args) => self.handle_stack_trace(seq, args),
            Command::Scopes(args) => self.handle_scopes(seq, args),
            Command::Variables(args) => self.handle_variables(seq, args),
            // Resume commands (Continue/Step) are handled specially by the
            // run_dap main loop: the response is sent BEFORE calling
            // resume_execution, so VS Code can transition to "running" state
            // immediately and flip the play/pause button. The main loop
            // detects these via is_resume_command() and calls handle_resume().
            Command::Continue(_) => {
                ok(seq, ResponseBody::Continue(dap::responses::ContinueResponse {
                    all_threads_continued: Some(true),
                }))
            }
            Command::Next(_) => ok(seq, ResponseBody::Next),
            Command::StepIn(_) => ok(seq, ResponseBody::StepIn),
            Command::StepOut(_) => ok(seq, ResponseBody::StepOut),
            Command::Pause(_) => {
                if let Some(ref mut vm) = self.vm {
                    vm.debug_mut().pause();
                }
                ok(seq, ResponseBody::Pause)
            }
            Command::Evaluate(args) => self.handle_evaluate(seq, args),
            Command::Disconnect(_) => {
                self.should_exit = true;
                ok(seq, ResponseBody::Disconnect)
            }
            _ => Response {
                request_seq: seq,
                success: true,
                message: None,
                body: None,
                error: None,
            },
        }
    }

    /// Load and parse the project — detects multi-file projects by walking
    /// up from the source file to find plc-project.yaml.
    /// Build a native FB registry from device profiles discovered in the project.
    /// Caches the result in `self.native_fb_registry` for reuse by both compile and VM.
    fn build_native_fb_registry(&mut self) -> Option<st_comm_api::NativeFbRegistry> {
        let path = std::path::Path::new(&self.source_path);
        let start = if path.is_file() { path.parent().unwrap_or(path) } else { path };
        let mut check = start.to_path_buf();
        let project_root = loop {
            if check.join("plc-project.yaml").exists() || check.join("plc-project.yml").exists() {
                break Some(check);
            }
            if !check.pop() { break None; }
        };

        let root = project_root?;
        let profiles_dir = root.join("profiles");
        if !profiles_dir.is_dir() {
            return None;
        }

        let mut registry = st_comm_api::NativeFbRegistry::new();
        let entries = std::fs::read_dir(&profiles_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" { continue; }
            if let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) {
                let name = profile.name.clone();
                registry.register(Box::new(
                    st_comm_sim::SimulatedNativeFb::new(&name, profile),
                ));
            }
        }

        if registry.is_empty() {
            None
        } else {
            let arc = std::sync::Arc::new(registry);
            self.native_fb_registry = Some(std::sync::Arc::clone(&arc));
            // Return a fresh registry for compile (can't return &Arc contents across borrow).
            // Rebuild cheaply from the same profiles — only happens once per debug session.
            let mut reg2 = st_comm_api::NativeFbRegistry::new();
            let entries2 = std::fs::read_dir(root.join("profiles")).ok()?;
            for entry in entries2.flatten() {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "yaml" && ext != "yml" { continue; }
                if let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) {
                    let name = profile.name.clone();
                    reg2.register(Box::new(
                        st_comm_sim::SimulatedNativeFb::new(&name, profile),
                    ));
                }
            }
            Some(reg2)
        }
    }

    fn load_project(&mut self) -> Result<st_syntax::lower::LowerResult, String> {
        let path = std::path::Path::new(&self.source_path);

        // Walk up from the file to find plc-project.yaml
        let project_root = {
            let start = if path.is_file() {
                path.parent().unwrap_or(path)
            } else {
                path
            };
            let mut check = start.to_path_buf();
            let mut found = None;
            loop {
                if check.join("plc-project.yaml").exists()
                    || check.join("plc-project.yml").exists()
                {
                    found = Some(check.clone());
                    break;
                }
                if !check.pop() {
                    break;
                }
            }
            found
        };

        if let Some(ref root) = project_root {
            // Load communication configuration BEFORE project discovery so the
            // auto-generated `_io_map.st` is on disk and gets picked up.
            self.comm_setup = comm_setup::load_for_project(root)
                .map_err(|e| format!("Comm config error: {e}"))?;
            if let Some(ref setup) = self.comm_setup {
                self.pending_events.push(console_output(&format!(
                    "Comm: {} link(s), {} device(s) — wrote {}",
                    setup.config.links.len(),
                    setup.config.devices.len(),
                    setup.io_map_path.display(),
                )));
                self.target_cycle_time = setup.engine.cycle_time;
            } else {
                // No comm devices, but the engine section may still exist.
                self.target_cycle_time = comm_setup::load_engine_config(root).cycle_time;
            }
            if let Some(ct) = self.target_cycle_time {
                self.pending_events.push(console_output(&format!(
                    "Engine cycle time: {ct:?}"
                )));
                // Compute telemetry emission interval so updates arrive
                // roughly every 500ms regardless of cycle_time. For a 10ms
                // cycle that's every 50 cycles; for a 100ms cycle, every 5.
                // Floor at 1 so we always emit at least once per cycle.
                let target_ms = ct.as_millis().max(1) as u32;
                self.cycle_event_interval = (500 / target_ms).max(1);
            }

            // Multi-file project mode — pass the project ROOT directory
            let project = st_syntax::project::discover_project(Some(root))
                .map_err(|e| format!("Project discovery failed: {e}"))?;

            self.pending_events.push(console_output(&format!(
                "Project '{}': {} source file(s)", project.name, project.source_files.len()
            )));

            let sources = st_syntax::project::load_project_sources(&project)
                .map_err(|e| format!("Cannot load sources: {e}"))?;

            // Store the main file source for breakpoint/stack-trace line mapping.
            // Find the file that matches self.source_path (the file the debugger was
            // launched from), or the first file containing a PROGRAM.
            let source_path_canonical = std::fs::canonicalize(&self.source_path).ok();
            for (path, content) in &sources {
                let path_canonical = std::fs::canonicalize(path).ok();
                if path_canonical == source_path_canonical {
                    self.source = content.clone();
                    break;
                }
            }
            // Fallback: use the last file (usually main.st, since files are sorted)
            if self.source.is_empty() {
                if let Some((_, content)) = sources.last() {
                    self.source = content.clone();
                }
            }

            // Store project files for function→file mapping later
            self.project_files = sources.iter()
                .map(|(p, c)| (p.to_string_lossy().to_string(), c.clone()))
                .collect();

            let stdlib = st_syntax::multi_file::builtin_stdlib();
            let mut all_sources: Vec<&str> = stdlib.to_vec();
            // The comm globals come from `_io_map.st` which is on disk and
            // already part of `sources` via project autodiscovery.
            let source_paths: Vec<String> = sources.iter()
                .map(|(p, _)| p.to_string_lossy().to_string())
                .collect();
            let owned: Vec<String> = sources.into_iter().map(|(_, content)| content).collect();

            // Compute per-file virtual offsets before pushing sources.
            // Offset = sum of all preceding source lengths.
            let stdlib_len: usize = all_sources.iter().map(|s| s.len()).sum();
            let mut cumulative = stdlib_len;
            self.file_virtual_offsets.clear();
            for (i, s) in owned.iter().enumerate() {
                if i < source_paths.len() {
                    self.file_virtual_offsets.insert(source_paths[i].clone(), cumulative);
                }
                cumulative += s.len();
            }

            for s in &owned {
                all_sources.push(s.as_str());
            }

            if let Some(ref ep) = project.entry_point {
                self.entry_point_override = Some(ep.clone());
            }

            Ok(st_syntax::multi_file::parse_multi(&all_sources))
        } else {
            // Single-file mode
            self.source = std::fs::read_to_string(&self.source_path)
                .map_err(|e| format!("Cannot read '{}': {e}", self.source_path))?;

            // Include stdlib
            let stdlib = st_syntax::multi_file::builtin_stdlib();
            let mut all_sources: Vec<&str> = stdlib.to_vec();
            let stdlib_offset: usize = all_sources.iter().map(|s| s.len()).sum();
            self.file_virtual_offsets.clear();
            self.file_virtual_offsets
                .insert(self.source_path.clone(), stdlib_offset);
            all_sources.push(&self.source);

            Ok(st_syntax::multi_file::parse_multi(&all_sources))
        }
    }

    fn handle_launch(&mut self, seq: i64) -> Response {
        self.pending_events.push(console_output(&format!(
            "Loading: {}", self.source_path
        )));

        // Try to discover a multi-file project from the source path.
        // Walk up from the file to find a plc-project.yaml, or detect a directory.
        let parse_result = match self.load_project() {
            Ok(result) => result,
            Err(msg) => return err(seq, &msg),
        };

        if !parse_result.errors.is_empty() {
            let msg = format!(
                "Cannot launch: {} parse error(s) — fix the errors shown in the Problems panel (Ctrl+Shift+M)",
                parse_result.errors.len()
            );
            self.pending_events.push(console_output(&msg));
            return err(seq, &msg);
        }

        // Build native FB registry from device profiles (if any).
        let native_registry = self.build_native_fb_registry();
        let module = match st_compiler::compile_with_native_fbs(
            &parse_result.source_file,
            native_registry.as_ref(),
        ) {
            Ok(m) => m,
            Err(e) => return err(seq, &format!("Compilation error: {e}")),
        };

        let func_count = module.functions.len();
        let instr_count: usize = module.functions.iter().map(|f| f.instructions.len()).sum();
        self.pending_events.push(console_output(&format!(
            "Compiled: {func_count} POU(s), {instr_count} instructions"
        )));

        // Build function → source file mapping for multi-file stack traces.
        // Parse each project file individually to find which top-level names it defines,
        // then map compiled function names to their defining file.
        self.func_source_map.clear();
        if !self.project_files.is_empty() {
            // Build name → (path, content) by parsing each file for its top-level declarations
            let mut name_to_file: std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new();
            for (path, content) in &self.project_files {
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
                        st_syntax::ast::TopLevelItem::Interface(iface) =>
                            vec![iface.name.name.clone()],
                        _ => vec![],
                    };
                    for name in names {
                        name_to_file.insert(
                            name.to_uppercase(),
                            (path.clone(), content.clone()),
                        );
                    }
                }
            }
            // Map compiled functions to their source file
            for func in &module.functions {
                if let Some(entry) = name_to_file.get(&func.name.to_uppercase()) {
                    self.func_source_map.insert(func.name.clone(), entry.clone());
                }
            }
        }

        if !module.functions.iter().any(|f| f.kind == PouKind::Program) {
            return err(seq, "No PROGRAM found in source file(s)");
        }

        let config = VmConfig {
            max_instructions: 100_000_000,
            ..Default::default()
        };
        let mut vm = Vm::new_with_native_fbs(module, config, self.native_fb_registry.clone());
        vm.debug_mut().resume(StepMode::StepIn, 0);

        // Register simulated devices and start their web UIs (if a comm setup
        // was loaded from plc-project.yaml).
        if let Some(ref mut setup) = self.comm_setup {
            for dev_cfg in &setup.config.devices {
                let Some(profile) = setup.profiles.get(&dev_cfg.device_profile) else {
                    continue;
                };
                if dev_cfg.protocol != "simulated" {
                    self.pending_events.push(console_output(&format!(
                        "[COMM] Skipping device '{}': protocol '{}' not implemented",
                        dev_cfg.name, dev_cfg.protocol
                    )));
                    continue;
                }
                let sim_device =
                    st_comm_sim::SimulatedDevice::new(&dev_cfg.name, profile.clone());
                let state_handle = sim_device.state_handle();
                let cycle_time = dev_cfg
                    .cycle_time
                    .as_ref()
                    .and_then(|s| st_comm_api::parse_duration(s).ok());
                let device_box: Box<dyn CommDevice> = Box::new(sim_device);
                self.comm.register_device(device_box, &dev_cfg.name, &vm, cycle_time);
                setup.device_states.push(comm_setup::DeviceState {
                    name: dev_cfg.name.clone(),
                    profile: profile.clone(),
                    state: state_handle,
                });
            }
            self.pending_events.push(console_output(&format!(
                "[COMM] Registered {} simulated device(s)",
                setup.device_states.len()
            )));
            comm_setup::start_web_uis(setup, 8080);
        }

        // Start the VM — it will immediately halt on the first instruction
        let program_name = self.entry_point_override.clone().unwrap_or_else(|| {
            vm.module()
                .functions
                .iter()
                .find(|f| f.kind == PouKind::Program)
                .map(|f| f.name.clone())
                .unwrap()
        });
        // Read device inputs into globals before the very first instruction halts.
        self.comm.read_inputs(&mut vm);
        vm.reset_instruction_count();
        // Do NOT stamp current_cycle_start here. The entry stop is a
        // debugger-only phase — the user may sit at it for seconds before
        // clicking Continue. If we stamped now, the first cycle's elapsed
        // time would include all that think-time. The actual cycle start
        // will be stamped by step_one_dap_iteration when execution begins.
        let _ = vm.run(&program_name); // Err(Halt) expected

        self.vm = Some(vm);

        self.pending_events.push(Event::Stopped(StoppedEventBody {
            reason: StoppedEventReason::Entry,
            description: Some("Stopped on entry".into()),
            thread_id: Some(1),
            preserve_focus_hint: None,
            text: None,
            all_threads_stopped: Some(true),
            hit_breakpoint_ids: None,
        }));

        // Push the variable catalog so the Monitor panel can populate its
        // autocomplete dropdown without having to receive every variable's
        // value on every cycle.
        self.push_var_catalog_event();

        // Start the embedded WebSocket monitor server on a random local port.
        // The VS Code extension picks up the port from the DAP event below
        // and connects the Monitor panel to ws://localhost:{port}.
        self.start_monitor_server();

        ok(seq, ResponseBody::Launch)
    }

    fn handle_set_breakpoints(
        &mut self,
        seq: i64,
        args: &dap::requests::SetBreakpointsArguments,
    ) -> Response {
        let mut breakpoints = Vec::new();

        // Pre-compute source content and virtual offsets BEFORE borrowing
        // self.vm mutably, since resolve_virtual_offset borrows self.
        let bp_source_path = args.source.path.as_ref().map(|p| p.to_string());
        let bp_source_content = if let Some(ref path) = bp_source_path {
            self.project_files.iter()
                .find(|(p, _)| {
                    let p_canon = std::fs::canonicalize(p).ok();
                    let bp_canon = std::fs::canonicalize(path).ok();
                    p_canon.is_some() && p_canon == bp_canon
                })
                .map(|(_, content)| content.clone())
                .or_else(|| std::fs::read_to_string(path).ok())
                .unwrap_or_else(|| self.source.clone())
        } else {
            self.source.clone()
        };

        if let Some(ref source_bps) = args.breakpoints {
            let lines: Vec<u32> = source_bps.iter().map(|bp| bp.line as u32).collect();

            // Store pending breakpoints per file path
            if let Some(ref path) = bp_source_path {
                self.pending_breakpoints.insert(path.clone(), (bp_source_content.clone(), lines.clone()));
            } else {
                self.pending_breakpoints.insert(self.source_path.clone(), (bp_source_content.clone(), lines.clone()));
            }

            // Pre-compute all virtual offsets
            let mut bp_offsets: Vec<(String, String, Vec<u32>, usize)> = Vec::new();
            for (path, (source, file_lines)) in &self.pending_breakpoints {
                let voff = self.resolve_virtual_offset(Some(path));
                bp_offsets.push((path.clone(), source.clone(), file_lines.clone(), voff));
            }
            let bp_voff = self.resolve_virtual_offset(bp_source_path.as_deref());

            if let Some(ref mut vm) = self.vm {
                let module = vm.module().clone();

                eprintln!("[DAP] SetBreakpoints: file={:?} lines={lines:?}, source len={}",
                    bp_source_path, bp_source_content.len());

                vm.debug_mut().clear_breakpoints();

                // Apply ALL accumulated breakpoints with their virtual offsets
                for (_, source, file_lines, voff) in &bp_offsets {
                    vm.debug_mut().set_line_breakpoints(&module, source, file_lines, *voff);
                }

                // Report results for THIS file's breakpoints
                let results = vm.debug_mut().set_line_breakpoints(&module, &bp_source_content, &lines, bp_voff);
                eprintln!("[DAP] Breakpoint results: {results:?}");

                for (i, result) in results.iter().enumerate() {
                    breakpoints.push(Breakpoint {
                        id: Some(i as i64 + 1),
                        verified: result.is_some(),
                        line: Some(source_bps[i].line),
                        ..Default::default()
                    });
                }
            }
        }

        ok(seq, ResponseBody::SetBreakpoints(
            dap::responses::SetBreakpointsResponse { breakpoints },
        ))
    }

    fn handle_stack_trace(&self, seq: i64, _args: &dap::requests::StackTraceArguments) -> Response {
        let mut stack_frames = Vec::new();

        if let Some(ref vm) = self.vm {
            for (i, frame) in vm.stack_frames().iter().enumerate() {
                // Resolve the correct source file for this function
                let (source_text, source_path, source_name) =
                    if let Some((path, content)) = self.func_source_map.get(&frame.func_name) {
                        (
                            content.as_str(),
                            path.clone(),
                            std::path::Path::new(path)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        )
                    } else {
                        (
                            self.source.as_str(),
                            self.source_path.clone(),
                            std::path::Path::new(&self.source_path)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        )
                    };

                // frame.source_offset is in virtual space — subtract
                // the file's virtual offset to get file-local byte offset.
                let voff = self.file_virtual_offsets.get(&source_path)
                    .or_else(|| {
                        let canon = std::fs::canonicalize(&source_path).ok()?;
                        self.file_virtual_offsets.iter()
                            .find(|(k, _)| std::fs::canonicalize(k).ok().as_ref() == Some(&canon))
                            .map(|(_, v)| v)
                    })
                    .copied()
                    .unwrap_or(0);
                let local_offset = frame.source_offset.saturating_sub(voff);
                let (line, _) = byte_offset_to_line_col(source_text, local_offset);
                stack_frames.push(StackFrame {
                    id: i as i64,
                    name: frame.func_name.clone(),
                    source: Some(Source {
                        name: Some(source_name),
                        path: Some(source_path),
                        ..Default::default()
                    }),
                    line: line as i64,
                    column: 1,
                    ..Default::default()
                });
            }
        }

        ok(seq, ResponseBody::StackTrace(dap::responses::StackTraceResponse {
            stack_frames,
            total_frames: None,
        }))
    }

    fn handle_scopes(&mut self, seq: i64, _args: &dap::requests::ScopesArguments) -> Response {
        let locals_ref = self.next_var_ref;
        let globals_ref = self.next_var_ref + 1;
        self.next_var_ref += 2;

        // Track which reference IDs map to which scope
        self.scope_refs.insert(locals_ref, ScopeKind::Locals);
        self.scope_refs.insert(globals_ref, ScopeKind::Globals);

        ok(seq, ResponseBody::Scopes(dap::responses::ScopesResponse {
            scopes: vec![
                Scope {
                    name: "Locals".into(),
                    presentation_hint: Some(ScopePresentationhint::Locals),
                    variables_reference: locals_ref,
                    ..Default::default()
                },
                Scope {
                    name: "Globals".into(),
                    presentation_hint: Some(ScopePresentationhint::Registers),
                    variables_reference: globals_ref,
                    ..Default::default()
                },
            ],
        }))
    }

    fn handle_variables(&mut self, seq: i64, args: &dap::requests::VariablesArguments) -> Response {
        let mut variables = Vec::new();
        let ref_id = args.variables_reference;

        if let Some(ref vm) = self.vm {
            // Check if this is a request for FB instance children.
            if let Some(fb_ref) = self.fb_var_refs.get(&ref_id).copied() {
                let fb_func = &vm.module().functions[fb_ref.fb_func_idx as usize];
                let instance_key = (fb_ref.caller_id, fb_ref.slot_idx);
                let fb_state = vm.fb_instances_ref().get(&instance_key);

                for (j, fb_slot) in fb_func.locals.slots.iter().enumerate() {
                    let fb_value = fb_state
                        .and_then(|s| s.get(j))
                        .cloned()
                        .unwrap_or(st_ir::Value::Void);

                    // Nested FBs get their own variablesReference for further expansion.
                    let child_ref = if let st_ir::VarType::FbInstance(nested_idx) = fb_slot.ty {
                        let id = self.next_var_ref;
                        self.next_var_ref += 1;
                        // caller_id for nested = (parent_slot << 16) | parent_fb_func
                        let nested_caller =
                            ((fb_ref.slot_idx as u32) << 16) | (fb_ref.fb_func_idx as u32);
                        self.fb_var_refs.insert(
                            id,
                            FbVarRef {
                                caller_id: nested_caller,
                                slot_idx: j as u16,
                                fb_func_idx: nested_idx,
                            },
                        );
                        id
                    } else {
                        0
                    };

                    variables.push(Variable {
                        name: fb_slot.name.clone(),
                        value: st_engine::debug::format_value(&fb_value),
                        type_field: Some(
                            st_engine::debug::format_var_type_with_width(
                                fb_slot.ty,
                                fb_slot.int_width,
                            )
                            .to_string(),
                        ),
                        variables_reference: child_ref,
                        ..Default::default()
                    });
                }
                return ok(
                    seq,
                    ResponseBody::Variables(dap::responses::VariablesResponse { variables }),
                );
            }

            // Check if this is a request for struct instance children.
            if let Some(sr) = self.struct_var_refs.get(&ref_id).copied() {
                if let Some((_, fields)) = vm.struct_type_fields(sr.type_def_idx) {
                    let instance_key = (sr.caller_id, sr.slot_idx);
                    let state = vm.fb_instances_ref().get(&instance_key);
                    for (j, field) in fields.iter().enumerate() {
                        let value = state
                            .and_then(|s| s.get(j))
                            .cloned()
                            .unwrap_or(st_ir::Value::default_for_type(field.ty));
                        variables.push(Variable {
                            name: field.name.clone(),
                            value: st_engine::debug::format_value(&value),
                            type_field: Some(
                                st_engine::debug::format_var_type_with_width(
                                    field.ty,
                                    field.int_width,
                                )
                                .to_string(),
                            ),
                            variables_reference: 0,
                            ..Default::default()
                        });
                    }
                }
                return ok(
                    seq,
                    ResponseBody::Variables(dap::responses::VariablesResponse { variables }),
                );
            }

            // Normal scope-based dispatch (Locals or Globals).
            let scope_kind = self.scope_refs.get(&ref_id).copied().unwrap_or(ScopeKind::Locals);

            if matches!(scope_kind, ScopeKind::Locals) {
                // Build locals with FB instances as expandable tree nodes.
                if let Some(frame) = vm.stack_frames().first() {
                    let func_index = frame.func_index;
                    let func = &vm.module().functions[func_index as usize];
                    let caller_id = vm.caller_identity_pub();

                    for (i, slot) in func.locals.slots.iter().enumerate() {
                        if let st_ir::VarType::FbInstance(fb_idx) = slot.ty {
                            // FB instance → expandable node with variablesReference.
                            let fb_ref_id = self.next_var_ref;
                            self.next_var_ref += 1;
                            self.fb_var_refs.insert(
                                fb_ref_id,
                                FbVarRef {
                                    caller_id,
                                    slot_idx: i as u16,
                                    fb_func_idx: fb_idx,
                                },
                            );
                            let fb_func = &vm.module().functions[fb_idx as usize];
                            // Build a summary value like "CTU (CV=2, Q=FALSE)"
                            let summary = self.fb_summary_value(vm, caller_id, i as u16, fb_idx);
                            variables.push(Variable {
                                name: slot.name.clone(),
                                value: summary,
                                type_field: Some(fb_func.name.clone()),
                                variables_reference: fb_ref_id,
                                ..Default::default()
                            });
                        } else if let st_ir::VarType::Struct(td_idx) = slot.ty {
                            // Struct variable → expandable node with variablesReference.
                            let struct_ref_id = self.next_var_ref;
                            self.next_var_ref += 1;
                            self.struct_var_refs.insert(
                                struct_ref_id,
                                StructVarRef {
                                    caller_id,
                                    slot_idx: i as u16,
                                    type_def_idx: td_idx,
                                },
                            );
                            // Build a summary and type name from the struct's type_def
                            let (type_name, summary) = if let Some((name, _)) = vm.struct_type_fields(td_idx) {
                                let sum = self.struct_summary_value(vm, caller_id, i as u16, td_idx);
                                (name.to_string(), sum)
                            } else {
                                ("STRUCT".to_string(), "(no fields)".to_string())
                            };
                            variables.push(Variable {
                                name: slot.name.clone(),
                                value: summary,
                                type_field: Some(type_name),
                                variables_reference: struct_ref_id,
                                ..Default::default()
                            });
                        } else if matches!(slot.ty, st_ir::VarType::ClassInstance(_)) {
                            // Skip class instances for now
                        } else {
                            let value = vm.current_locals()
                                .iter()
                                .find(|v| v.name.eq_ignore_ascii_case(&slot.name))
                                .map(|v| v.value.clone())
                                .unwrap_or_else(|| "?".to_string());
                            variables.push(Variable {
                                name: slot.name.clone(),
                                value,
                                type_field: Some(
                                    st_engine::debug::format_var_type_with_width(
                                        slot.ty,
                                        slot.int_width,
                                    )
                                    .to_string(),
                                ),
                                variables_reference: 0,
                                ..Default::default()
                            });
                        }
                    }
                }
            } else {
                let vars = vm.global_variables();
                for v in vars {
                    variables.push(Variable {
                        name: v.name,
                        value: v.value,
                        type_field: Some(v.ty),
                        variables_reference: 0,
                        ..Default::default()
                    });
                }
            }
        }

        ok(seq, ResponseBody::Variables(dap::responses::VariablesResponse { variables }))
    }

    fn handle_evaluate(&mut self, seq: i64, args: &dap::requests::EvaluateArguments) -> Response {
        let expr = args.expression.trim();

        // Handle PLC-specific commands via evaluate expressions
        // force <var> = <value>
        if let Some(rest) = expr.strip_prefix("force ") {
            return self.handle_force_command(seq, rest);
        }
        // unforce <var>
        if let Some(var_name) = expr.strip_prefix("unforce ") {
            return self.handle_unforce_command(seq, var_name.trim());
        }
        // scanCycleInfo
        if expr.eq_ignore_ascii_case("scanCycleInfo") || expr.eq_ignore_ascii_case("cycleinfo") {
            return self.handle_cycle_info(seq);
        }
        // listForced
        if expr.eq_ignore_ascii_case("listForced") || expr.eq_ignore_ascii_case("forced") {
            return self.handle_list_forced(seq);
        }
        // watchVariables a,b,c — replace the watch list (comma-separated)
        if let Some(rest) = expr.strip_prefix("watchVariables ") {
            return self.handle_watch_replace(seq, rest);
        }
        // addWatch <var>
        if let Some(rest) = expr.strip_prefix("addWatch ") {
            return self.handle_watch_add(seq, rest.trim());
        }
        // removeWatch <var>
        if let Some(rest) = expr.strip_prefix("removeWatch ") {
            return self.handle_watch_remove(seq, rest.trim());
        }
        // clearWatch
        if expr.eq_ignore_ascii_case("clearWatch") {
            self.watched_variables.clear();
            return self.eval_text_response(seq, "Watch list cleared");
        }
        // varCatalog — request a fresh catalog of all monitorable vars
        if expr.eq_ignore_ascii_case("varCatalog") {
            self.push_var_catalog_event();
            return self.eval_text_response(seq, "Catalog pushed");
        }

        // Normal variable lookup: first check locals + globals by exact name,
        // then fall back to monitorable_variables which includes FB instance
        // fields like `Main.filler.counter.Q`. For dotted expressions like
        // `counter.Q` typed in the debug console while paused inside a FB,
        // we also try qualifying with the current POU name.
        let mut result_str = "<unknown>".to_string();
        let mut var_ref: i64 = 0;
        let mut type_name: Option<String> = None;

        if let Some(ref vm) = self.vm {
            // First: check if the expression matches a FB instance local
            // in the current frame. If so, return it as expandable.
            if let Some(frame) = vm.stack_frames().first() {
                let func = &vm.module().functions[frame.func_index as usize];
                let caller_id = vm.caller_identity_pub();
                if let Some((slot_idx, slot)) = func.locals.find_slot(expr) {
                    if let st_ir::VarType::FbInstance(fb_idx) = slot.ty {
                        // FB instance → expandable in the Watch panel
                        let ref_id = self.next_var_ref;
                        self.next_var_ref += 1;
                        self.fb_var_refs.insert(
                            ref_id,
                            FbVarRef {
                                caller_id,
                                slot_idx,
                                fb_func_idx: fb_idx,
                            },
                        );
                        let fb_func = &vm.module().functions[fb_idx as usize];
                        result_str = self.fb_summary_value(vm, caller_id, slot_idx, fb_idx);
                        var_ref = ref_id;
                        type_name = Some(fb_func.name.clone());
                    } else if let st_ir::VarType::Struct(td_idx) = slot.ty {
                        // Struct instance → expandable in the Watch panel
                        let ref_id = self.next_var_ref;
                        self.next_var_ref += 1;
                        self.struct_var_refs.insert(
                            ref_id,
                            StructVarRef {
                                caller_id,
                                slot_idx,
                                type_def_idx: td_idx,
                            },
                        );
                        let tn = vm.struct_type_fields(td_idx)
                            .map(|(name, _)| name.to_string())
                            .unwrap_or_else(|| "STRUCT".to_string());
                        result_str = self.struct_summary_value(vm, caller_id, slot_idx, td_idx);
                        var_ref = ref_id;
                        type_name = Some(tn);
                    } else {
                        // Scalar local — look up via current_locals()
                        let locals = vm.current_locals();
                        if let Some(v) = locals.iter().find(|v| v.name.eq_ignore_ascii_case(expr)) {
                            result_str = v.value.clone();
                            type_name = Some(v.ty.clone());
                        }
                    }
                }
            }

            // If not found in locals, try globals + FB field paths
            if result_str == "<unknown>" {
                let globals = vm.global_variables();
                if let Some(v) = globals
                    .iter()
                    .find(|v| v.name.eq_ignore_ascii_case(expr))
                {
                    result_str = v.value.clone();
                    type_name = Some(v.ty.clone());
                } else if expr.contains('.') {
                    // Dotted path: try resolving as a FB instance field
                    if let Some(v) = vm.resolve_fb_field(expr) {
                        result_str = v.value;
                        type_name = Some(v.ty);
                    } else {
                        let all = vm.monitorable_variables();
                        if let Some(v) = all
                            .iter()
                            .find(|v| {
                                v.name.eq_ignore_ascii_case(expr)
                                    || v.name.to_uppercase().ends_with(
                                        &format!(".{}", expr.to_uppercase()),
                                    )
                            })
                        {
                            result_str = v.value.clone();
                            type_name = Some(v.ty.clone());
                        }
                    }
                }
            }
        }

        ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
            result: result_str,
            type_field: type_name,
            presentation_hint: None,
            variables_reference: var_ref,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
        }))
    }

    /// Helper: build a textual evaluate response.
    fn eval_text_response(&self, seq: i64, text: &str) -> Response {
        ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
            result: text.to_string(),
            type_field: None,
            presentation_hint: None,
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
        }))
    }

    fn handle_watch_replace(&mut self, seq: i64, csv: &str) -> Response {
        let names: Vec<String> = csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let count = names.len();
        self.watched_variables = names;
        // Push a fresh telemetry snapshot so the panel shows the new
        // watch list immediately rather than waiting for the next tick.
        self.push_cycle_stats_event();
        self.eval_text_response(seq, &format!("Watching {count} variable(s)"))
    }

    fn handle_watch_add(&mut self, seq: i64, name: &str) -> Response {
        if name.is_empty() {
            return self.eval_text_response(seq, "Usage: addWatch <variable>");
        }
        if !self
            .watched_variables
            .iter()
            .any(|v| v.eq_ignore_ascii_case(name))
        {
            self.watched_variables.push(name.to_string());
        }
        self.push_cycle_stats_event();
        self.eval_text_response(seq, &format!("Now watching '{name}'"))
    }

    fn handle_watch_remove(&mut self, seq: i64, name: &str) -> Response {
        if name.is_empty() {
            return self.eval_text_response(seq, "Usage: removeWatch <variable>");
        }
        let before = self.watched_variables.len();
        self.watched_variables
            .retain(|v| !v.eq_ignore_ascii_case(name));
        let removed = before - self.watched_variables.len();
        self.push_cycle_stats_event();
        if removed > 0 {
            self.eval_text_response(seq, &format!("Stopped watching '{name}'"))
        } else {
            self.eval_text_response(seq, &format!("'{name}' was not in the watch list"))
        }
    }

    /// Push a `plc/varCatalog` telemetry event listing every monitorable
    /// variable's name and type. Used by the Monitor panel to populate
    /// its autocomplete dropdown without having to receive every variable
    /// value on every cycle.
    fn push_var_catalog_event(&mut self) {
        let catalog: Vec<serde_json::Value> = self
            .vm
            .as_ref()
            .map(|vm| {
                vm.monitorable_catalog()
                    .iter()
                    .map(|(name, ty)| {
                        serde_json::json!({
                            "name": name,
                            "type": ty,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let payload = serde_json::json!({
            "schema": 1,
            "variables": catalog,
        });
        self.pending_events.push(Event::Output(OutputEventBody {
            category: Some(OutputEventCategory::Telemetry),
            output: "plc/varCatalog".to_string(),
            data: Some(payload),
            ..Default::default()
        }));
    }

    // ── Embedded Monitor WS Server ──────────────────────────────────

    /// Start the embedded WebSocket monitor server on a random local port.
    /// Populates the catalog from the current VM and sends the port to the
    /// VS Code extension via a DAP telemetry event.
    fn start_monitor_server(&mut self) {
        let handle = st_monitor::MonitorHandle::new();

        // Populate catalog from the VM
        if let Some(ref vm) = self.vm {
            let catalog: Vec<st_monitor::CatalogEntry> = vm
                .monitorable_catalog()
                .into_iter()
                .map(|(name, ty)| st_monitor::CatalogEntry {
                    name,
                    var_type: ty,
                })
                .collect();
            eprintln!("[DAP-MONITOR] Catalog: {} variables", catalog.len());
            handle.set_catalog(catalog);
        }

        // Start the server on a background tokio runtime (the DAP main loop
        // is synchronous — no tokio runtime on this thread).
        let server_handle = handle.clone();
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("dap-monitor".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build monitor tokio runtime");
                rt.block_on(async {
                    match st_monitor::run_monitor_server("127.0.0.1:0", server_handle).await {
                        Ok(addr) => {
                            let _ = port_tx.send(addr.port());
                            eprintln!("[DAP-MONITOR] WS server listening on {addr}");
                            // Keep the runtime alive until the process exits
                            std::future::pending::<()>().await;
                        }
                        Err(e) => {
                            eprintln!("[DAP-MONITOR] Failed to start: {e}");
                            let _ = port_tx.send(0);
                        }
                    }
                });
            })
            .expect("Failed to spawn monitor thread");

        // Wait for the port (with a short timeout)
        if let Ok(port) = port_rx.recv_timeout(std::time::Duration::from_secs(2)) {
            if port > 0 {
                eprintln!("[DAP-MONITOR] Sending monitor port {port} to VS Code");
                self.pending_events.push(Event::Output(OutputEventBody {
                    category: Some(OutputEventCategory::Telemetry),
                    output: "plc/monitorPort".to_string(),
                    data: Some(serde_json::json!({ "port": port })),
                    ..Default::default()
                }));
            }
        }

        self.monitor_handle = Some(handle);
    }

    /// Push variable snapshot + cycle stats to the monitor server.
    /// Called after each completed scan cycle.
    fn push_monitor_snapshot(&self) {
        let Some(ref handle) = self.monitor_handle else { return };
        if !handle.has_subscribers() {
            return;
        }
        let Some(ref vm) = self.vm else { return };

        let forced = vm.forced_variables();
        let all_vars = vm.monitorable_variables();
        let vars: Vec<st_monitor::VariableValue> = all_vars
            .into_iter()
            .map(|v| {
                let is_forced = forced.contains_key(&v.name.to_uppercase());
                st_monitor::VariableValue {
                    name: v.name,
                    value: v.value,
                    var_type: v.ty,
                    forced: is_forced,
                }
            })
            .collect();

        let cs = &self.cycle_stats;
        let cycle_info = st_monitor::CycleInfoData {
            cycle_count: cs.cycle_count,
            last_cycle_us: cs.last_cycle_time.as_micros() as u64,
            min_cycle_us: if cs.min_cycle_time == Duration::MAX {
                0
            } else {
                cs.min_cycle_time.as_micros() as u64
            },
            max_cycle_us: cs.max_cycle_time.as_micros() as u64,
            avg_cycle_us: cs.avg_cycle_time().as_micros() as u64,
            target_cycle_us: self.target_cycle_time.map(|d| d.as_micros() as u64).unwrap_or(0),
            last_period_us: cs.last_cycle_period.as_micros() as u64,
            min_period_us: if cs.min_cycle_period == Duration::MAX {
                0
            } else {
                cs.min_cycle_period.as_micros() as u64
            },
            max_period_us: cs.max_cycle_period.as_micros() as u64,
            jitter_max_us: cs.jitter_max.as_micros() as u64,
        };

        handle.update_variables(vars, cycle_info);
    }

    /// Apply forced variables and stats resets from WS monitor clients.
    /// Called between scan cycles.
    fn apply_monitor_commands(&mut self) {
        let Some(ref handle) = self.monitor_handle else { return };

        // Apply forced variables
        let forces = handle.take_forced_variables();
        if !forces.is_empty() {
            if let Some(ref mut vm) = self.vm {
                for (name, value) in forces {
                    eprintln!("[DAP-MONITOR] Applying force: {name} = {value:?}");
                    vm.force_variable(&name, value);
                }
            }
        }

        // Reset stats if requested
        if handle.take_reset_stats() {
            eprintln!("[DAP-MONITOR] Resetting cycle stats");
            self.cycle_stats.min_cycle_time = Duration::MAX;
            self.cycle_stats.max_cycle_time = Duration::ZERO;
            self.cycle_stats.min_cycle_period = Duration::MAX;
            self.cycle_stats.max_cycle_period = Duration::ZERO;
            self.cycle_stats.jitter_max = Duration::ZERO;
        }
    }

    fn handle_force_command(&mut self, seq: i64, expr: &str) -> Response {
        // Parse "varname = value"
        let parts: Vec<&str> = expr.splitn(2, '=').collect();
        if parts.len() != 2 {
            return ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
                result: "Usage: force <variable> = <value>".into(),
                type_field: None, presentation_hint: None,
                variables_reference: 0, named_variables: None,
                indexed_variables: None, memory_reference: None,
            }));
        }
        let var_name = parts[0].trim();
        let value_str = parts[1].trim();

        let value = if value_str.eq_ignore_ascii_case("true") {
            st_ir::Value::Bool(true)
        } else if value_str.eq_ignore_ascii_case("false") {
            st_ir::Value::Bool(false)
        } else if let Ok(i) = value_str.parse::<i64>() {
            st_ir::Value::Int(i)
        } else if let Ok(f) = value_str.parse::<f64>() {
            st_ir::Value::Real(f)
        } else {
            st_ir::Value::String(value_str.to_string())
        };

        if self.vm.is_some() {
            self.vm.as_mut().unwrap().force_variable(var_name, value.clone());
            let result = format!("Forced {} = {}", var_name, st_engine::debug::format_value(&value));
            self.pending_events.push(console_output(&result));
            // Push a fresh telemetry snapshot so the Monitor panel updates
            // immediately rather than waiting for the next periodic tick.
            // Without this, the user sees the value pop in 100-500ms later.
            self.push_cycle_stats_event();
            ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
                result,
                type_field: None, presentation_hint: None,
                variables_reference: 0, named_variables: None,
                indexed_variables: None, memory_reference: None,
            }))
        } else {
            ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
                result: "No program running".into(),
                type_field: None, presentation_hint: None,
                variables_reference: 0, named_variables: None,
                indexed_variables: None, memory_reference: None,
            }))
        }
    }

    fn handle_unforce_command(&mut self, seq: i64, var_name: &str) -> Response {
        if self.vm.is_some() {
            self.vm.as_mut().unwrap().unforce_variable(var_name);
            let result = format!("Unforced {var_name}");
            self.pending_events.push(console_output(&result));
            // Push a fresh telemetry snapshot so the panel's lock icon
            // disappears immediately on unforce.
            self.push_cycle_stats_event();
            ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
                result,
                type_field: None, presentation_hint: None,
                variables_reference: 0, named_variables: None,
                indexed_variables: None, memory_reference: None,
            }))
        } else {
            ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
                result: "No program running".into(),
                type_field: None, presentation_hint: None,
                variables_reference: 0, named_variables: None,
                indexed_variables: None, memory_reference: None,
            }))
        }
    }

    fn handle_cycle_info(&self, seq: i64) -> Response {
        let result = if self.vm.is_some() {
            let s = &self.cycle_stats;
            let to_us = |d: Duration| d.as_micros();
            let min_us = if s.min_cycle_time == Duration::MAX {
                0
            } else {
                to_us(s.min_cycle_time)
            };
            let watchdog = match self.watchdog_us {
                Some(w) => format!("{w}µs"),
                None => "disabled".into(),
            };
            let jitter = if s.jitter_max > Duration::ZERO {
                format!("{}µs", to_us(s.jitter_max))
            } else if self.target_cycle_time.is_some() {
                "0µs".into()
            } else {
                "n/a (no cycle_time target)".into()
            };
            let period = if s.last_cycle_period > Duration::ZERO {
                let min_p = if s.min_cycle_period == Duration::MAX {
                    0
                } else {
                    to_us(s.min_cycle_period)
                };
                format!(
                    " | period: {}µs (min/max: {}µs / {}µs)",
                    to_us(s.last_cycle_period),
                    min_p,
                    to_us(s.max_cycle_period),
                )
            } else {
                String::new()
            };
            format!(
                "Scan cycles: {} | Instructions/cycle: {} | last: {}µs | min/max/avg: {}µs / {}µs / {}µs | jitter: {jitter}{period} | watchdog: {}",
                s.cycle_count,
                self.current_cycle_instructions,
                to_us(s.last_cycle_time),
                min_us,
                to_us(s.max_cycle_time),
                to_us(s.avg_cycle_time()),
                watchdog,
            )
        } else {
            "No program running".into()
        };

        ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
            result,
            type_field: None, presentation_hint: None,
            variables_reference: 0, named_variables: None,
            indexed_variables: None, memory_reference: None,
        }))
    }

    fn handle_list_forced(&self, seq: i64) -> Response {
        let result = if let Some(ref vm) = self.vm {
            let forced = vm.forced_variables();
            if forced.is_empty() {
                "No forced variables".into()
            } else {
                forced.iter()
                    .map(|(name, val)| format!("{} = {}", name, st_engine::debug::format_value(val)))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        } else {
            "No program running".into()
        };

        ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
            result,
            type_field: None, presentation_hint: None,
            variables_reference: 0, named_variables: None,
            indexed_variables: None, memory_reference: None,
        }))
    }

    /// Apply a completed cycle's elapsed time + instruction count into
    /// `cycle_stats`. Called after the resume loop releases its borrows on
    /// `self.vm` and `self.comm`. If the periodic threshold is reached, also
    /// pushes a `plc/cycleStats` telemetry event.
    fn record_completed_cycle(
        &mut self,
        elapsed: Duration,
        instructions: u64,
        cycle_started: Instant,
    ) {
        self.cycle_stats.cycle_count += 1;
        self.cycle_stats.last_cycle_time = elapsed;
        self.cycle_stats.total_time += elapsed;
        if elapsed < self.cycle_stats.min_cycle_time {
            self.cycle_stats.min_cycle_time = elapsed;
        }
        if elapsed > self.cycle_stats.max_cycle_time {
            self.cycle_stats.max_cycle_time = elapsed;
        }
        self.current_cycle_instructions = instructions;

        // Period tracking: wall-clock interval between consecutive cycle starts.
        if let Some(prev) = self.previous_cycle_start {
            let period = cycle_started.duration_since(prev);
            self.cycle_stats.last_cycle_period = period;
            if period < self.cycle_stats.min_cycle_period {
                self.cycle_stats.min_cycle_period = period;
            }
            if period > self.cycle_stats.max_cycle_period {
                self.cycle_stats.max_cycle_period = period;
            }
            if let Some(target) = self.target_cycle_time {
                let dev = period.abs_diff(target);
                if dev > self.cycle_stats.jitter_max {
                    self.cycle_stats.jitter_max = dev;
                }
            }
        }
        self.previous_cycle_start = Some(cycle_started);

        self.cycles_since_last_event = self.cycles_since_last_event.saturating_add(1);
        if self.cycle_event_interval > 0
            && self.cycles_since_last_event >= self.cycle_event_interval
        {
            self.cycles_since_last_event = 0;
            self.push_cycle_stats_event();
        }

        // Push to the embedded WS monitor server (if any clients are connected)
        self.push_monitor_snapshot();
    }

    /// Push a `plc/cycleStats` telemetry event reflecting the latest stats,
    /// including a snapshot of *watched* variable values for the Monitor
    /// panel. Variables outside `watched_variables` are NOT included — the
    /// panel must opt-in via `addWatch` so we don't ship hundreds of values
    /// every 500ms in projects with lots of I/O points.
    fn push_cycle_stats_event(&mut self) {
        let (devices_ok, devices_err) = self.comm.health_counts();
        let variables: Vec<serde_json::Value> = if self.watched_variables.is_empty() {
            Vec::new()
        } else {
            self.vm
                .as_ref()
                .map(|vm| {
                    let all = vm.monitorable_variables();
                    let forced = vm.forced_variables();
                    let mut result = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for name in &self.watched_variables {
                        let upper = name.to_uppercase();
                        let prefix = format!("{upper}.");

                        // Collect ALL descendants under this prefix.
                        let mut descendants = Vec::new();
                        for v in &all {
                            if v.name.to_uppercase().starts_with(&prefix)
                                && seen.insert(v.name.to_uppercase())
                            {
                                let is_forced = forced.contains_key(&v.name.to_uppercase());
                                descendants.push(serde_json::json!({
                                    "name": v.name,
                                    "value": v.value,
                                    "type": v.ty,
                                    "forced": is_forced,
                                }));
                            }
                        }

                        if descendants.is_empty() {
                            // Scalar variable — exact match only.
                            if let Some(v) =
                                all.iter().find(|v| v.name.eq_ignore_ascii_case(name))
                            {
                                if seen.insert(v.name.to_uppercase()) {
                                    let is_forced =
                                        forced.contains_key(&v.name.to_uppercase());
                                    result.push(serde_json::json!({
                                        "name": v.name,
                                        "value": v.value,
                                        "type": v.ty,
                                        "forced": is_forced,
                                    }));
                                }
                            }
                        } else {
                            // FB instance — build nested children tree from
                            // flat dotted-path descendants.
                            let exact = all
                                .iter()
                                .find(|v| v.name.eq_ignore_ascii_case(name));
                            let parent_type = exact.map(|v| v.ty.as_str()).unwrap_or("");
                            let parent_value =
                                exact.map(|v| v.value.as_str()).unwrap_or("");
                            let children =
                                Self::build_children_tree(name, &descendants);
                            result.push(serde_json::json!({
                                "name": name,
                                "value": parent_value,
                                "type": parent_type,
                                "children": children,
                            }));
                            // Also emit flat descendants so existing panel
                            // code (prefix matching) still works.
                            result.extend(descendants);
                        }
                    }
                    result
                })
                .unwrap_or_default()
        };
        self.pending_events.push(cycle_stats_event(
            &self.cycle_stats,
            self.current_cycle_instructions,
            devices_ok,
            devices_err,
            self.watchdog_us,
            self.target_cycle_time,
            &variables,
        ));
    }

    /// Build a nested children tree from flat dotted-path descendants.
    ///
    /// Given prefix "Main.filler" and flat entries like:
    ///   Main.filler.start (BOOL), Main.filler.counter.CV (INT), ...
    /// produces:
    ///   [ { name: "start", value: "FALSE", type: "BOOL" },
    ///     { name: "counter", children: [ { name: "CV", ... } ] } ]
    fn build_children_tree(
        prefix: &str,
        flat: &[serde_json::Value],
    ) -> Vec<serde_json::Value> {
        use std::collections::BTreeMap;

        // Group by first path segment after the prefix.
        struct Node {
            /// Direct value (if this segment has one).
            value: Option<serde_json::Value>,
            /// Sub-entries keyed by next segment.
            children: BTreeMap<String, Node>,
        }

        let prefix_dot = format!("{prefix}.");
        let mut root = BTreeMap::<String, Node>::new();

        for entry in flat {
            let full_name = entry["name"].as_str().unwrap_or("");
            let Some(relative) = full_name
                .get(prefix_dot.len()..)
                .filter(|_| full_name.len() > prefix_dot.len())
            else {
                continue;
            };
            let parts: Vec<&str> = relative.split('.').collect();
            // Insert into the tree, walking each segment.
            fn insert_at(
                map: &mut BTreeMap<String, Node>,
                parts: &[&str],
                entry: &serde_json::Value,
            ) {
                if parts.is_empty() {
                    return;
                }
                let key = parts[0].to_string();
                let node = map.entry(key).or_insert_with(|| Node {
                    value: None,
                    children: BTreeMap::new(),
                });
                if parts.len() == 1 {
                    node.value = Some(entry.clone());
                } else {
                    insert_at(&mut node.children, &parts[1..], entry);
                }
            }
            insert_at(&mut root, &parts, entry);
        }

        fn to_json(map: &BTreeMap<String, Node>) -> Vec<serde_json::Value> {
            let mut out = Vec::new();
            for (key, node) in map {
                if node.children.is_empty() {
                    // Leaf node
                    if let Some(ref v) = node.value {
                        let mut obj = serde_json::json!({
                            "name": key,
                            "value": v["value"],
                            "type": v["type"],
                        });
                        if v["forced"].as_bool() == Some(true) {
                            obj["forced"] = serde_json::json!(true);
                        }
                        out.push(obj);
                    }
                } else {
                    // Intermediate node (FB instance)
                    let children = to_json(&node.children);
                    let mut obj = serde_json::json!({
                        "name": key,
                        "children": children,
                    });
                    // If this intermediate node also has a direct value
                    // (e.g., from the flat list), include it.
                    if let Some(ref v) = node.value {
                        obj["value"] = v["value"].clone();
                        obj["type"] = v["type"].clone();
                    }
                    out.push(obj);
                }
            }
            out
        }

        to_json(&root)
    }

    /// One iteration of the DAP run loop. Either continues an in-flight
    /// program (vm.continue_execution) or starts a fresh scan cycle. The
    /// borrow split on `self.vm`/`self.comm` is scoped to this function so
    /// the outer loop has full `&mut self` access between calls.
    ///
    /// `cycle_start` tracks the wall-clock at which the *currently in-flight*
    /// scan cycle began. It is `Some` while a cycle is mid-execution
    /// (possibly across multiple breakpoint pauses) and `None` between
    /// cycles. A fresh-cycle iteration stamps it; a completed-cycle iteration
    /// consumes it.
    fn step_one_dap_iteration(
        &mut self,
        mode: StepMode,
        program_name: &str,
        cycle_start: &mut Option<Instant>,
    ) -> CycleStep {
        let comm = &mut self.comm;
        let vm = self.vm.as_mut().unwrap();

        let result = if vm.call_depth() > 0 {
            // Continuing from a previous halt (breakpoint, entry stop, step).
            // Ensure cycle_start is set so we measure actual VM time, not
            // user think-time since the Stopped event.
            if cycle_start.is_none() {
                *cycle_start = Some(Instant::now());
            }
            vm.continue_execution()
        } else {
            // Start of a fresh scan cycle: pull device inputs into VM globals.
            comm.read_inputs(vm);
            // Only reset the debug state if there's no pending pause — otherwise
            // resume_with_source would overwrite step_mode=Paused back to
            // Continue, swallowing the user's Pause request.
            if vm.debug_state().step_mode != StepMode::Paused {
                vm.debug_mut().resume_with_source(mode, 0, 0);
            }
            vm.reset_instruction_count();
            let now = Instant::now();
            *cycle_start = Some(now);
            vm.run(program_name)
        };

        match result {
            Err(VmError::Halt) => CycleStep::Halted,
            Ok(_) => {
                comm.write_outputs(vm);
                let started = cycle_start.take().unwrap_or_else(Instant::now);
                let elapsed = started.elapsed();
                let instructions = vm.instruction_count();
                CycleStep::Completed { elapsed, instructions, cycle_started: started }
            }
            Err(e) => CycleStep::Error(format!("{e}")),
        }
    }

    /// Build the textual `func_name line N` description for the topmost
    /// stack frame. Used in Stopped events and console output.
    fn current_frame_description(&self) -> String {
        let Some(ref vm) = self.vm else {
            return "<unknown>".to_string();
        };
        vm.stack_frames()
            .first()
            .map(|f| {
                let (src, src_path) = self
                    .func_source_map
                    .get(&f.func_name)
                    .map(|(p, c)| (c.as_str(), p.as_str()))
                    .unwrap_or((&self.source, &self.source_path));
                let voff = self
                    .file_virtual_offsets
                    .get(src_path)
                    .copied()
                    .unwrap_or(0);
                let local_offset = f.source_offset.saturating_sub(voff);
                let (line, _) = byte_offset_to_line_col(src, local_offset);
                format!("{} line {line}", f.func_name)
            })
            .unwrap_or_else(|| "<unknown>".to_string())
    }

    /// Push a Stopped event reflecting the VM's current pause reason.
    fn push_halt_stopped_event(&mut self) {
        let pause_reason = self
            .vm
            .as_ref()
            .map(|vm| vm.debug_state().pause_reason)
            .unwrap_or(PauseReason::None);
        let frame_desc = self.current_frame_description();
        eprintln!("[DAP] Halted: reason={pause_reason:?} at {frame_desc}");

        let reason = match pause_reason {
            PauseReason::Breakpoint => StoppedEventReason::Breakpoint,
            PauseReason::Step => StoppedEventReason::Step,
            PauseReason::PauseRequest => StoppedEventReason::Pause,
            PauseReason::Entry => StoppedEventReason::Entry,
            PauseReason::None => StoppedEventReason::Step,
        };
        self.pending_events.push(console_output(&format!(
            "Stopped: {pause_reason:?} at {frame_desc}"
        )));
        self.pending_events.push(Event::Stopped(StoppedEventBody {
            reason,
            description: Some(format!("Stopped at {frame_desc}")),
            thread_id: Some(1),
            preserve_focus_hint: None,
            text: None,
            all_threads_stopped: Some(true),
            hit_breakpoint_ids: None,
        }));
    }

    /// Push a runtime error message + Terminated event.
    fn push_runtime_error(&mut self, msg: &str) {
        eprintln!("[DAP] Runtime error: {msg}");
        self.pending_events.push(Event::Output(OutputEventBody {
            category: Some(OutputEventCategory::Stderr),
            output: format!("Runtime error: {msg}\n"),
            ..Default::default()
        }));
        self.pending_events.push(Event::Terminated(None));
    }

    /// Drain incoming requests from the channel without blocking. Pause is
    /// applied inline (sets the VM's pause flag). Disconnect is reported via
    /// the return value. SetBreakpoints is applied inline so a breakpoint
    /// added during a long Continue takes effect immediately. All other
    /// requests are queued onto `self.deferred_requests` for the outer
    /// `run_dap` loop to process after `resume_execution` returns.
    fn process_inflight_requests(&mut self) -> InflightAction {
        let mut action = InflightAction::default();

        // Drain everything currently buffered. We don't loop blocking — the
        // outer run loop calls this between cycles only.
        while let Some(req) = self
            .request_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            eprintln!("[DAP] Inflight request: {:?}", req.command);
            match &req.command {
                Command::Pause(_) => {
                    if let Some(ref mut vm) = self.vm {
                        vm.debug_mut().pause();
                    }
                    action.pause_requested = true;
                    self.deferred_requests.push(req);
                }
                Command::Disconnect(_) => {
                    action.disconnect_requested = true;
                    self.deferred_requests.push(req);
                }
                Command::SetBreakpoints(args) => {
                    let response = self.handle_set_breakpoints(req.seq, args);
                    self.deferred_responses.push(response);
                }
                Command::Evaluate(args) => {
                    // Handle evaluate inline so the Monitor panel's
                    // addWatch / removeWatch / force / unforce commands take
                    // effect WHILE the program is running, not after the next
                    // pause. The mutating watch commands also push a fresh
                    // cycle stats event so the panel updates immediately.
                    let response = self.handle_evaluate(req.seq, args);
                    self.deferred_responses.push(response);
                }
                _ => {
                    self.deferred_requests.push(req);
                }
            }
        }
        action
    }

    /// Sleep for `target` while remaining responsive to incoming requests.
    /// Returns true if a Pause/Disconnect was processed during the sleep
    /// (caller should bail out of its run loop).
    fn interruptible_sleep(&mut self, target: Duration) -> bool {
        const CHUNK: Duration = Duration::from_millis(10);
        let deadline = Instant::now() + target;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline - now;
            let chunk = if remaining < CHUNK { remaining } else { CHUNK };
            std::thread::sleep(chunk);
            let action = self.process_inflight_requests();
            if action.disconnect_requested || action.pause_requested {
                return true;
            }
        }
    }

    fn resume_execution<W: Write>(&mut self, mode: StepMode, writer: &mut DapWriter<W>) {
        if self.vm.is_none() {
            eprintln!("[DAP] resume_execution: no VM");
            return;
        }

        // Per the DAP spec, variable references issued via the previous
        // `Scopes` request become invalid as soon as execution resumes. Drop
        // them now so the HashMap doesn't grow unboundedly across thousands
        // of pause/resume cycles in a long debug session. The next `Scopes`
        // request after the next Stopped event will allocate fresh refs.
        self.scope_refs.clear();
        self.fb_var_refs.clear();

        // Setup: tell the VM what stepping mode we're in and where we are.
        let (depth, current_source_offset, program_name) = {
            let vm = self.vm.as_mut().unwrap();
            let depth = vm.call_depth();
            let offset = vm
                .stack_frames()
                .first()
                .map(|f| f.source_offset)
                .unwrap_or(0);
            vm.debug_mut().resume_with_source(mode, depth, offset);
            let program_name = vm
                .module()
                .functions
                .iter()
                .find(|f| f.kind == PouKind::Program)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            (depth, offset, program_name)
        };
        eprintln!(
            "[DAP] resume: mode={mode:?} depth={depth} source_offset={current_source_offset}"
        );

        let mut cycle_start = self.current_cycle_start.take();
        let mut current_mode = mode;
        let mut any_completed = false;
        let mut cycles_in_this_resume: u64 = 0;

        // Safety cap: even though Continue is meant to run "until the user
        // stops", we don't want a runaway loop in a test or a malformed
        // program to consume unbounded RAM (pending_events grows during the
        // run since nothing drains it until we return). Ten million cycles
        // is ~10s at 1µs/cycle — well above any interactive use, low enough
        // to fail fast in CI.
        const SAFETY_CYCLE_CAP: u64 = 10_000_000;

        loop {
            // Step first, then check for inflight requests. The opposite
            // order would mean a queued Disconnect (which is the very first
            // thing the test harness puts in the channel after the run
            // request) would short-circuit the run before any work happens —
            // breaking step requests entirely.
            let outcome =
                self.step_one_dap_iteration(current_mode, &program_name, &mut cycle_start);

            match outcome {
                CycleStep::Halted => {
                    self.push_halt_stopped_event();
                    break;
                }
                CycleStep::Completed { elapsed, instructions, cycle_started } => {
                    self.record_completed_cycle(elapsed, instructions, cycle_started);
                    any_completed = true;
                    cycles_in_this_resume = cycles_in_this_resume.saturating_add(1);

                    // Flush accumulated events (telemetry, console output) to
                    // the wire NOW so the VS Code status bar and PLC Monitor
                    // webview update in real time while the program runs,
                    // rather than buffering everything until the loop exits.
                    // Stopped / Terminated events are pushed just before
                    // `break` and won't be in pending_events at this point.
                    for event in self.pending_events.drain(..) {
                        let _ = writer.send_event(event);
                    }

                    if current_mode != StepMode::Continue {
                        // Step-mode wrap-around: the user stepped past the
                        // end of the program. Run one more iteration in
                        // StepIn mode to halt at the first statement of the
                        // next cycle, matching the behavior of every other
                        // PLC IDE.
                        current_mode = StepMode::StepIn;
                        continue;
                    }

                    // Apply forced variables from WS monitor clients
                    self.apply_monitor_commands();

                    // Continue mode only: drain incoming requests so the
                    // run loop is interruptible by Pause/Disconnect/SetBp.
                    let action = self.process_inflight_requests();
                    if action.pause_requested {
                        // We're between scan cycles, so there's no in-flight
                        // execution to halt — vm.pause() was called by
                        // process_inflight_requests, and the next iteration's
                        // fresh-cycle branch would clear the pause flag via
                        // resume_with_source. Push the Stopped(Pause) event
                        // here directly to acknowledge the request.
                        self.push_halt_stopped_event();
                        break;
                    }
                    if action.disconnect_requested {
                        eprintln!("[DAP] Disconnect during run loop — exiting");
                        break;
                    }

                    // Enforce the cycle period. When no cycle_time is
                    // configured, default to 1ms — free-spinning at max CPU
                    // is never useful for debugging and starves the reader
                    // thread (making Pause/Disconnect unreliable).
                    const DEFAULT_CYCLE: Duration = Duration::from_millis(1);
                    let target = self.target_cycle_time.unwrap_or(DEFAULT_CYCLE);
                    if let Some(remaining) = target.checked_sub(elapsed) {
                        if !remaining.is_zero() && self.interruptible_sleep(remaining) {
                            // A Pause/Disconnect arrived during the sleep.
                            continue;
                        }
                    }

                    if cycles_in_this_resume >= SAFETY_CYCLE_CAP {
                        eprintln!(
                            "[DAP] Safety cap reached ({SAFETY_CYCLE_CAP} cycles in one resume) — terminating"
                        );
                        self.pending_events.push(console_output(
                            "[DAP] Safety cycle cap reached — exiting run loop",
                        ));
                        self.pending_events.push(Event::Terminated(None));
                        break;
                    }
                }
                CycleStep::Error(msg) => {
                    self.push_runtime_error(&msg);
                    break;
                }
            }
        }

        // Preserve any in-flight cycle start so the next resume_execution
        // call (e.g., resuming from a breakpoint) folds the full cycle's
        // timing into stats.
        self.current_cycle_start = cycle_start;
        // Reset period tracker so user-pause time between the Stopped event
        // and the next Continue doesn't pollute period / jitter measurements.
        self.previous_cycle_start = None;

        // Force a fresh status-bar update on every interactive boundary
        // (breakpoint hit / step) so feedback is instant rather than waiting
        // for the periodic interval to elapse.
        if any_completed {
            self.push_cycle_stats_event();
        }
    }
}

/// Outcome of one iteration of the DAP run loop.
enum CycleStep {
    /// VM halted (breakpoint, step done, pause request, etc.). Outer loop
    /// pushes a Stopped event and breaks.
    Halted,
    /// One scan cycle ran to completion. Outer loop applies stats, then
    /// either keeps running (Continue mode) or wraps around to the next
    /// cycle and halts at the first statement (Step modes).
    Completed {
        elapsed: Duration,
        instructions: u64,
        /// When this cycle started (for period / jitter tracking).
        cycle_started: Instant,
    },
    /// Runtime error from the VM. Outer loop pushes Terminated and breaks.
    Error(String),
}

/// Side-effects of draining incoming requests during a run loop.
#[derive(Default)]
struct InflightAction {
    pause_requested: bool,
    disconnect_requested: bool,
}

fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut col = 1;
    for (i, b) in source.bytes().enumerate() {
        if i >= offset { break; }
        if b == b'\n' { line += 1; col = 1; } else { col += 1; }
    }
    (line, col)
}
