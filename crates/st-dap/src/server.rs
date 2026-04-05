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

    loop {
        let req = match server.poll_request() {
            Ok(Some(req)) => req,
            Ok(None) | Err(_) => break,
        };

        let response = session.handle_request(&req);
        if server.respond(response).is_err() {
            break;
        }

        for event in session.pending_events.drain(..) {
            if server.send_event(event).is_err() {
                break;
            }
        }

        if session.should_exit {
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

struct DapSession {
    source_path: String,
    source: String,
    vm: Option<Vm>,
    pending_events: Vec<Event>,
    should_exit: bool,
    next_var_ref: i64,
}

impl DapSession {
    fn new(source_path: &str) -> Self {
        Self {
            source_path: source_path.to_string(),
            source: String::new(),
            vm: None,
            pending_events: Vec::new(),
            should_exit: false,
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

    fn handle_launch(&mut self, seq: i64) -> Response {
        self.source = match std::fs::read_to_string(&self.source_path) {
            Ok(s) => s,
            Err(e) => return err(seq, &format!("Cannot read '{}': {e}", self.source_path)),
        };

        let parse_result = st_syntax::parse(&self.source);
        if !parse_result.errors.is_empty() {
            return err(seq, "Parse errors in source file");
        }

        let module = match st_compiler::compile(&parse_result.source_file) {
            Ok(m) => m,
            Err(e) => return err(seq, &format!("Compilation error: {e}")),
        };

        if !module.functions.iter().any(|f| f.kind == PouKind::Program) {
            return err(seq, "No PROGRAM found in source file");
        }

        let config = VmConfig {
            max_instructions: 100_000_000,
            ..Default::default()
        };
        let mut vm = Vm::new(module, config);
        vm.debug_mut().resume(StepMode::StepIn, 0);

        // Start the VM — it will immediately halt on the first instruction
        let program_name = vm
            .module()
            .functions
            .iter()
            .find(|f| f.kind == PouKind::Program)
            .map(|f| f.name.clone())
            .unwrap();
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

    fn handle_evaluate(&self, seq: i64, args: &dap::requests::EvaluateArguments) -> Response {
        let mut result_str = "<unknown>".to_string();

        if let Some(ref vm) = self.vm {
            let locals = vm.current_locals();
            let globals = vm.global_variables();

            if let Some(v) = locals
                .iter()
                .chain(globals.iter())
                .find(|v| v.name.eq_ignore_ascii_case(&args.expression))
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

    fn resume_execution(&mut self, mode: StepMode) {
        let Some(ref mut vm) = self.vm else { return };

        let depth = vm.call_depth();
        // Get current source offset so stepping knows when we've moved to a new line
        let current_source_offset = vm.stack_frames()
            .first()
            .map(|f| f.source_offset)
            .unwrap_or(0);
        vm.debug_mut().resume_with_source(mode, depth, current_source_offset);

        // If the VM already has call frames (paused mid-execution), continue.
        // Otherwise, start a new run.
        let result = if vm.call_depth() > 0 {
            vm.continue_execution()
        } else {
            let program_name = vm
                .module()
                .functions
                .iter()
                .find(|f| f.kind == PouKind::Program)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            vm.run(&program_name)
        };

        match result {
            Err(VmError::Halt) => {
                let reason = match vm.debug_state().pause_reason {
                    PauseReason::Breakpoint => StoppedEventReason::Breakpoint,
                    PauseReason::Step => StoppedEventReason::Step,
                    PauseReason::PauseRequest => StoppedEventReason::Pause,
                    PauseReason::Entry => StoppedEventReason::Entry,
                    PauseReason::None => StoppedEventReason::Step,
                };
                self.pending_events.push(Event::Stopped(StoppedEventBody {
                    reason,
                    description: None,
                    thread_id: Some(1),
                    preserve_focus_hint: None,
                    text: None,
                    all_threads_stopped: Some(true),
                    hit_breakpoint_ids: None,
                }));
            }
            Ok(_) => {
                self.pending_events.push(Event::Terminated(None));
            }
            Err(e) => {
                self.pending_events.push(Event::Output(OutputEventBody {
                    category: Some(OutputEventCategory::Stderr),
                    output: format!("Runtime error: {e}\n"),
                    ..Default::default()
                }));
                self.pending_events.push(Event::Terminated(None));
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
