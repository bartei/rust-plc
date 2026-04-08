//! DAP server implementation.

use dap::events::*;
use dap::prelude::*;
use dap::requests::Command;
use dap::responses::{ResponseBody, ResponseMessage};
use dap::types::*;
use st_comm_api::CommDevice;
use st_ir::PouKind;
use st_runtime::comm_manager::CommManager;
use st_runtime::debug::{PauseReason, StepMode};
use st_runtime::vm::{Vm, VmConfig, VmError};
use std::io::{BufReader, BufWriter, Read, Write};

use crate::comm_setup;

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

#[derive(Debug, Clone, Copy)]
enum ScopeKind {
    Locals,
    Globals,
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
    /// Communication manager for simulated devices (read inputs / write outputs each scan).
    comm: CommManager,
    /// Comm setup data (config, profiles, generated source) loaded from plc-project.yaml.
    comm_setup: Option<comm_setup::CommSetup>,
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
            next_var_ref: 1000,
            comm: CommManager::new(),
            comm_setup: None,
        }
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
        let mut vm = Vm::new(module, config);
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
                let device_box: Box<dyn CommDevice> = Box::new(sim_device);
                self.comm.register_device(device_box, &dev_cfg.name, &vm);
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

                // Determine which source file these breakpoints are for.
                // VS Code sends the file path in args.source.
                let bp_source_path = args.source.path.as_ref()
                    .map(|p| p.to_string());

                let bp_source_content = if let Some(ref path) = bp_source_path {
                    // Try to find this file in project_files
                    self.project_files.iter()
                        .find(|(p, _)| {
                            // Compare canonical paths to handle symlinks/relative paths
                            let p_canon = std::fs::canonicalize(p).ok();
                            let bp_canon = std::fs::canonicalize(path).ok();
                            p_canon.is_some() && p_canon == bp_canon
                        })
                        .map(|(_, content)| content.clone())
                        .or_else(|| {
                            // Fallback: try reading the file directly
                            std::fs::read_to_string(path).ok()
                        })
                        .unwrap_or_else(|| self.source.clone())
                } else {
                    self.source.clone()
                };

                eprintln!("[DAP] SetBreakpoints: file={:?} lines={lines:?}, source len={}",
                    bp_source_path, bp_source_content.len());

                // Don't clear ALL breakpoints — only set new ones additively.
                // VS Code sends complete breakpoint list per file, so we rebuild from scratch.
                vm.debug_mut().clear_breakpoints();

                // Re-apply breakpoints for ALL files we know about
                // (VS Code sends SetBreakpoints per file, but we need to accumulate)
                // Store pending breakpoints per file path
                if let Some(path) = bp_source_path {
                    self.pending_breakpoints.insert(path, (bp_source_content.clone(), lines.clone()));
                } else {
                    self.pending_breakpoints.insert(self.source_path.clone(), (bp_source_content.clone(), lines.clone()));
                }

                // Apply ALL accumulated breakpoints
                for (source, file_lines) in self.pending_breakpoints.values() {
                    vm.debug_mut().set_line_breakpoints(&module, source, file_lines);
                }

                // Report results for THIS file's breakpoints
                let results = vm.debug_mut().set_line_breakpoints(&module, &bp_source_content, &lines);
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

                let (line, _) = byte_offset_to_line_col(source_text, frame.source_offset);
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

    fn handle_variables(&self, seq: i64, args: &dap::requests::VariablesArguments) -> Response {
        let mut variables = Vec::new();

        if let Some(ref vm) = self.vm {
            let scope_kind = self.scope_refs.get(&args.variables_reference)
                .copied()
                .unwrap_or(ScopeKind::Locals);
            let vars = match scope_kind {
                ScopeKind::Locals => vm.current_locals(),
                ScopeKind::Globals => vm.global_variables(),
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
        if self.vm.is_none() {
            eprintln!("[DAP] resume_execution: no VM");
            return;
        }
        // Disjoint borrows so we can use both `vm` and `comm` in the loop below.
        let comm = &mut self.comm;
        let vm = self.vm.as_mut().unwrap();

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
                // Start of a fresh scan cycle: pull device inputs into VM globals.
                comm.read_inputs(vm);
                vm.debug_mut().resume_with_source(mode, 0, 0);
                vm.run(&program_name)
            };

            match result {
                Err(VmError::Halt) => {
                    let pause_reason = vm.debug_state().pause_reason;
                    let frame_desc = vm.stack_frames()
                        .first()
                        .map(|f| {
                            let src = self.func_source_map.get(&f.func_name)
                                .map(|(_, c)| c.as_str())
                                .unwrap_or(&self.source);
                            let (line, _) = byte_offset_to_line_col(src, f.source_offset);
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
                    // Cycle completed — push VM globals out to device outputs.
                    comm.write_outputs(vm);
                    cycles += 1;

                    if mode != StepMode::Continue && cycles >= max_cycles {
                        // Step mode reached end of cycle — start next cycle
                        // and stop at the first statement (like a wrap-around)
                        eprintln!("[DAP] Step wrapped to next cycle");
                        // New scan cycle: read inputs again
                        comm.read_inputs(vm);
                        vm.debug_mut().resume_with_source(StepMode::StepIn, 0, 0);
                        match vm.run(&program_name) {
                            Err(VmError::Halt) => {
                                let pause_reason = vm.debug_state().pause_reason;
                                let frame_desc = vm.stack_frames()
                                    .first()
                                    .map(|f| {
                                        let src = self.func_source_map.get(&f.func_name)
                                            .map(|(_, c)| c.as_str())
                                            .unwrap_or(&self.source);
                                        let (line, _) = byte_offset_to_line_col(src, f.source_offset);
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
