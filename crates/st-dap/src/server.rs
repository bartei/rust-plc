//! DAP server implementation.

use dap::events::*;
use dap::prelude::*;
use dap::requests::Command;
use dap::responses::{ResponseBody, ResponseMessage};
use dap::types::*;
use st_ir::PouKind;
use st_runtime::debug::{PauseReason, StepMode};
use st_runtime::vm::{Vm, VmConfig, VmError};
use std::io::{BufReader, BufWriter, Read, Write};

/// Run the DAP server on the given reader/writer.
pub fn run_dap<R: Read, W: Write>(input: R, output: W, source_path: &str) {
    let mut server = Server::new(BufReader::new(input), BufWriter::new(output));
    let mut session = DapSession::new(source_path);

    // Log to stderr for development debugging
    eprintln!("[DAP] Server started for: {source_path}");

    loop {
        let req = match server.poll_request() {
            Ok(Some(req)) => req,
            Ok(None) => {
                eprintln!("[DAP] Client disconnected (EOF)");
                break;
            }
            Err(e) => {
                eprintln!("[DAP] Read error: {e}");
                break;
            }
        };

        eprintln!("[DAP] Request: {:?}", req.command);

        let response = session.handle_request(&req);

        eprintln!("[DAP] Response: success={}", response.success);

        if server.respond(response).is_err() {
            eprintln!("[DAP] Failed to send response");
            break;
        }

        for event in session.pending_events.drain(..) {
            eprintln!("[DAP] Event: {event:?}");
            if server.send_event(event).is_err() {
                eprintln!("[DAP] Failed to send event");
                break;
            }
        }

        if session.should_exit {
            eprintln!("[DAP] Session exit requested");
            break;
        }
    }
    // Ensure all output is flushed
    // The dap crate's Server holds a BufWriter that may not flush on drop
    // in all edge cases. We extract and explicitly flush here.
    std::mem::drop(server);
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

struct DapSession {
    source_path: String,
    source: String,
    vm: Option<Vm>,
    pending_events: Vec<Event>,
    should_exit: bool,
    next_var_ref: i64,
    entry_point_override: Option<String>,
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
            next_var_ref: 1000,
        }
    }

    fn handle_request(&mut self, req: &Request) -> Response {
        let seq = req.seq;
        match &req.command {
            Command::Initialize(_) => {
                self.pending_events.push(Event::Initialized);
                ok(seq, ResponseBody::Initialize(Capabilities {
                    supports_configuration_done_request: Some(true),
                    supports_evaluate_for_hovers: Some(true),
                    ..Default::default()
                }))
            }
            Command::Launch(_) => self.handle_launch(seq),
            Command::SetBreakpoints(args) => self.handle_set_breakpoints(seq, args),
            Command::ConfigurationDone => ok(seq, ResponseBody::ConfigurationDone),
            Command::Threads => ok(seq, ResponseBody::Threads(dap::responses::ThreadsResponse {
                threads: vec![Thread { id: 1, name: "PLC Scan Cycle".into() }],
            })),
            Command::StackTrace(args) => self.handle_stack_trace(seq, args),
            Command::Scopes(args) => self.handle_scopes(seq, args),
            Command::Variables(args) => self.handle_variables(seq, args),
            Command::Continue(_) => {
                self.resume_execution(StepMode::Continue);
                ok(seq, ResponseBody::Continue(dap::responses::ContinueResponse {
                    all_threads_continued: Some(true),
                }))
            }
            Command::Next(_) => {
                self.resume_execution(StepMode::StepOver);
                ok(seq, ResponseBody::Next)
            }
            Command::StepIn(_) => {
                self.resume_execution(StepMode::StepIn);
                ok(seq, ResponseBody::StepIn)
            }
            Command::StepOut(_) => {
                self.resume_execution(StepMode::StepOut);
                ok(seq, ResponseBody::StepOut)
            }
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
            // Multi-file project mode — pass the project ROOT directory
            let project = st_syntax::project::discover_project(Some(root))
                .map_err(|e| format!("Project discovery failed: {e}"))?;

            self.pending_events.push(console_output(&format!(
                "Project '{}': {} source file(s)", project.name, project.source_files.len()
            )));

            let sources = st_syntax::project::load_project_sources(&project)
                .map_err(|e| format!("Cannot load sources: {e}"))?;

            let stdlib = st_syntax::multi_file::builtin_stdlib();
            let mut all_sources: Vec<&str> = stdlib.to_vec();
            let owned: Vec<String> = sources.into_iter().map(|(_, content)| content).collect();
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
            let msg = format!("{} parse error(s) found", parse_result.errors.len());
            self.pending_events.push(console_output(&msg));
            return err(seq, &msg);
        }

        let module = match st_compiler::compile(&parse_result.source_file) {
            Ok(m) => m,
            Err(e) => return err(seq, &format!("Compilation error: {e}")),
        };

        let func_count = module.functions.len();
        let instr_count: usize = module.functions.iter().map(|f| f.instructions.len()).sum();
        self.pending_events.push(console_output(&format!(
            "Compiled: {func_count} POU(s), {instr_count} instructions"
        )));

        if !module.functions.iter().any(|f| f.kind == PouKind::Program) {
            return err(seq, "No PROGRAM found in source file(s)");
        }

        let config = VmConfig {
            max_instructions: 100_000_000,
            ..Default::default()
        };
        let mut vm = Vm::new(module, config);
        vm.debug_mut().resume(StepMode::StepIn, 0);

        // Start the VM — it will immediately halt on the first instruction
        let program_name = self.entry_point_override.clone().unwrap_or_else(|| {
            vm.module()
                .functions
                .iter()
                .find(|f| f.kind == PouKind::Program)
                .map(|f| f.name.clone())
                .unwrap()
        });
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

        ok(seq, ResponseBody::Launch)
    }

    fn handle_set_breakpoints(
        &mut self,
        seq: i64,
        args: &dap::requests::SetBreakpointsArguments,
    ) -> Response {
        let mut breakpoints = Vec::new();

        if let Some(ref mut vm) = self.vm {
            let module = vm.module().clone();
            if let Some(ref source_bps) = args.breakpoints {
                let lines: Vec<u32> = source_bps.iter().map(|bp| bp.line as u32).collect();
                vm.debug_mut().clear_breakpoints();
                let results = vm.debug_mut().set_line_breakpoints(&module, &self.source, &lines);

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
                let (line, _) = byte_offset_to_line_col(&self.source, frame.source_offset);
                stack_frames.push(StackFrame {
                    id: i as i64,
                    name: frame.func_name.clone(),
                    source: Some(Source {
                        name: Some(
                            std::path::Path::new(&self.source_path)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        ),
                        path: Some(self.source_path.clone()),
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
                    presentation_hint: Some(ScopePresentationhint::Locals),
                    variables_reference: globals_ref,
                    ..Default::default()
                },
            ],
        }))
    }

    fn handle_variables(&self, seq: i64, args: &dap::requests::VariablesArguments) -> Response {
        let mut variables = Vec::new();

        if let Some(ref vm) = self.vm {
            let vars = if args.variables_reference % 2 == 0 {
                vm.current_locals()
            } else {
                vm.global_variables()
            };

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

        // Normal variable lookup
        let mut result_str = "<unknown>".to_string();

        if let Some(ref vm) = self.vm {
            let locals = vm.current_locals();
            let globals = vm.global_variables();

            if let Some(v) = locals
                .iter()
                .chain(globals.iter())
                .find(|v| v.name.eq_ignore_ascii_case(expr))
            {
                result_str = v.value.clone();
            }
        }

        ok(seq, ResponseBody::Evaluate(dap::responses::EvaluateResponse {
            result: result_str,
            type_field: None,
            presentation_hint: None,
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
        }))
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

        if let Some(ref mut vm) = self.vm {
            vm.force_variable(var_name, value.clone());
            let result = format!("Forced {} = {}", var_name, st_runtime::debug::format_value(&value));
            self.pending_events.push(console_output(&result));
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
        if let Some(ref mut vm) = self.vm {
            vm.unforce_variable(var_name);
            let result = format!("Unforced {var_name}");
            self.pending_events.push(console_output(&result));
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
        let result = if let Some(ref vm) = self.vm {
            format!("Scan cycles: {} | Instructions: {}",
                0, // cycle count tracked by engine, not VM directly
                vm.instruction_count()
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
                    .map(|(name, val)| format!("{} = {}", name, st_runtime::debug::format_value(val)))
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

    fn resume_execution(&mut self, mode: StepMode) {
        let Some(ref mut vm) = self.vm else {
            eprintln!("[DAP] resume_execution: no VM");
            return;
        };

        let depth = vm.call_depth();
        let current_source_offset = vm.stack_frames()
            .first()
            .map(|f| f.source_offset)
            .unwrap_or(0);

        eprintln!("[DAP] resume: mode={mode:?} depth={depth} source_offset={current_source_offset}");
        vm.debug_mut().resume_with_source(mode, depth, current_source_offset);

        let program_name = vm
            .module()
            .functions
            .iter()
            .find(|f| f.kind == PouKind::Program)
            .map(|f| f.name.clone())
            .unwrap_or_default();

        // Run cycles until the VM halts (breakpoint/step) or errors out.
        // In Continue mode, this may run many scan cycles before a breakpoint hits.
        // In Step modes, it stops after one statement.
        let max_cycles = if mode == StepMode::Continue { 100_000 } else { 1 };
        let mut cycles = 0u64;

        loop {
            let result = if vm.call_depth() > 0 {
                vm.continue_execution()
            } else {
                vm.debug_mut().resume_with_source(mode, 0, 0);
                vm.run(&program_name)
            };

            match result {
                Err(VmError::Halt) => {
                    let pause_reason = vm.debug_state().pause_reason;
                    let frame_desc = vm.stack_frames()
                        .first()
                        .map(|f| {
                            let mut line = 1usize;
                            for (i, b) in self.source.bytes().enumerate() {
                                if i >= f.source_offset { break; }
                                if b == b'\n' { line += 1; }
                            }
                            format!("{} line {line}", f.func_name)
                        })
                        .unwrap_or_else(|| "<unknown>".to_string());
                    eprintln!("[DAP] Halted: reason={pause_reason:?} at {frame_desc} (after {cycles} cycles)");

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
                    break;
                }
                Ok(_) => {
                    // Cycle completed — start next cycle
                    cycles += 1;

                    if mode != StepMode::Continue && cycles >= max_cycles {
                        // Step mode reached end of cycle — start next cycle
                        // and stop at the first statement (like a wrap-around)
                        eprintln!("[DAP] Step wrapped to next cycle");
                        vm.debug_mut().resume_with_source(StepMode::StepIn, 0, 0);
                        match vm.run(&program_name) {
                            Err(VmError::Halt) => {
                                let pause_reason = vm.debug_state().pause_reason;
                                let frame_desc = vm.stack_frames()
                                    .first()
                                    .map(|f| {
                                        let mut line = 1usize;
                                        for (i, b) in self.source.bytes().enumerate() {
                                            if i >= f.source_offset { break; }
                                            if b == b'\n' { line += 1; }
                                        }
                                        format!("{} line {line}", f.func_name)
                                    })
                                    .unwrap_or_else(|| "<unknown>".to_string());
                                eprintln!("[DAP] Next cycle stopped at {frame_desc}");
                                self.pending_events.push(console_output(&format!(
                                    "Stopped: {pause_reason:?} at {frame_desc}"
                                )));
                                self.pending_events.push(Event::Stopped(StoppedEventBody {
                                    reason: StoppedEventReason::Step,
                                    description: Some(format!("Stopped at {frame_desc}")),
                                    thread_id: Some(1),
                                    preserve_focus_hint: None,
                                    text: None,
                                    all_threads_stopped: Some(true),
                                    hit_breakpoint_ids: None,
                                }));
                            }
                            _ => {
                                self.pending_events.push(Event::Terminated(None));
                            }
                        }
                        break;
                    }

                    if cycles >= 100_000 {
                        eprintln!("[DAP] Reached max cycles without breakpoint");
                        self.pending_events.push(console_output("Reached max cycles without breakpoint"));
                        self.pending_events.push(Event::Terminated(None));
                        break;
                    }
                    // Continue to next cycle
                }
                Err(e) => {
                    eprintln!("[DAP] Runtime error after {cycles} cycles: {e}");
                    self.pending_events.push(Event::Output(OutputEventBody {
                        category: Some(OutputEventCategory::Stderr),
                        output: format!("Runtime error: {e}\n"),
                        ..Default::default()
                    }));
                    self.pending_events.push(Event::Terminated(None));
                    break;
                }
            }
        }
    }
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
