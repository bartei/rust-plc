mod comm_setup;

use std::env;
use std::path::{Path, PathBuf};
use std::process;

/// Parse source with the standard library included.
fn parse_with_stdlib(source: &str) -> st_syntax::lower::LowerResult {
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all_sources: Vec<&str> = stdlib;
    all_sources.push(source);
    st_syntax::multi_file::parse_multi(&all_sources)
}

/// Load and parse all sources from a project (stdlib + project files).
fn parse_project(project: &st_syntax::project::Project) -> Result<st_syntax::lower::LowerResult, String> {
    let sources = st_syntax::project::load_project_sources(project)?;
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    let source_strs: Vec<&str> = sources.iter().map(|(_, s)| s.as_str()).collect();
    all.extend(&source_strs);
    Ok(st_syntax::multi_file::parse_multi(&all))
}


fn print_usage() {
    eprintln!("st-cli: IEC 61131-3 Structured Text toolchain");
    eprintln!();
    eprintln!("Usage: st-cli <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  serve             Start the LSP server (stdio)");
    eprintln!("  check [path]      Parse and analyze, report diagnostics");
    eprintln!("  run [path] [-n N] Compile and execute (N cycles, default 1)");
    eprintln!("  compile <path> -o <output>  Compile to bytecode file");
    eprintln!("  fmt [path]        Format source file(s) in place");
    eprintln!("  bundle [path]     Create a .st-bundle for deployment");
    eprintln!("  target list [path]  List configured deployment targets");
    eprintln!("  comm-gen [path]   Regenerate _io_map.st from plc-project.yaml + profiles");
    eprintln!("  debug <file>      Start DAP debug server (stdin/stdout)");
    eprintln!("  help              Show this help message");
    eprintln!();
    eprintln!("Flags:");
    eprintln!("  --json            Output diagnostics as JSON (for CI integration)");
    eprintln!();
    eprintln!("Path modes:");
    eprintln!("  (no path)         Use current directory as project root");
    eprintln!("  file.st           Single file mode");
    eprintln!("  directory/        Project mode (autodiscover .st files)");
    eprintln!("  plc-project.yaml  Explicit project file");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "serve" => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                )
                .with_writer(std::io::stderr)
                .init();

            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(st_lsp::run_stdio());
        }
        "check" => {
            let path = args.get(2).map(|s| s.as_str());
            run_check(path, &args);
        }
        "run" => {
            run_program_cmd(&args);
        }
        "compile" => {
            run_compile_cmd(&args);
        }
        "fmt" => {
            run_fmt_cmd(&args);
        }
        "bundle" => {
            run_bundle_cmd(&args);
        }
        "target" => {
            run_target_cmd(&args);
        }
        "comm-gen" => {
            let target = args.get(2).map(|s| s.as_str()).map(Path::new);
            let root = resolve_project_root(target);
            match comm_setup::load_for_project(&root) {
                Ok(Some(setup)) => {
                    eprintln!(
                        "Wrote {} ({} device(s), {} profile(s))",
                        setup.io_map_path.display(),
                        setup.config.devices.len(),
                        setup.profiles.len()
                    );
                }
                Ok(None) => {
                    eprintln!("No comm devices configured in {}", root.display());
                }
                Err(e) => {
                    eprintln!("Comm config error: {e}");
                    process::exit(1);
                }
            }
        }
        "debug" => {
            if args.len() < 3 {
                eprintln!("Usage: st-cli debug <file>");
                process::exit(1);
            }
            let path = &args[2];
            st_dap::run_dap(std::io::stdin(), std::io::stdout(), path);
        }
        "help" | "--help" | "-h" => {
            print_usage();
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            process::exit(1);
        }
    }
}

fn run_check(path: Option<&str>, args: &[String]) {
    let json_output = args.iter().any(|a| a == "--json");
    let target = path.filter(|p| *p != "--json").map(Path::new);

    // Determine if single file or project
    let is_single_file = target
        .map(|p| p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), Some("st" | "scl")))
        .unwrap_or(false);

    if is_single_file {
        let path = target.unwrap();
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {e}", path.display());
                process::exit(1);
            }
        };

        let parse_result = parse_with_stdlib(&source);
        let mut result = st_semantics::analyze::analyze(&parse_result.source_file);
        for err in &parse_result.errors {
            result.diagnostics.insert(0, st_semantics::diagnostic::Diagnostic::error(
                st_semantics::diagnostic::DiagnosticCode::UndeclaredVariable,
                err.message.clone(), err.range,
            ));
        }

        let has_errors = print_diagnostics(&result.diagnostics, &source, &path.display().to_string(), json_output);
        if !json_output && result.diagnostics.is_empty() {
            eprintln!("{}: OK", path.display());
        }
        if has_errors { process::exit(1); }
    } else {
        // Project mode

        // Refresh the auto-generated I/O map before discovering files,
        // so the LSP/check sees the same set of comm globals as `run`.
        let probe_root = resolve_project_root(target);
        if let Err(e) = comm_setup::load_for_project(&probe_root) {
            eprintln!("Comm config error: {e}");
            process::exit(1);
        }

        let project = match st_syntax::project::discover_project(target) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Project discovery error: {e}");
                process::exit(1);
            }
        };

        eprintln!("Project '{}': {} source file(s)", project.name, project.source_files.len());
        for f in &project.source_files {
            eprintln!("  {}", f.strip_prefix(&project.root).unwrap_or(f).display());
        }

        let parse_result = match parse_project(&project) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        };

        let result = st_semantics::analyze::analyze(&parse_result.source_file);
        let has_errors = print_diagnostics(&result.diagnostics, "", &project.name, json_output);

        if !json_output && !has_errors {
            eprintln!("Project '{}': OK", project.name);
        }
        if has_errors { process::exit(1); }
    }
}

fn run_program_cmd(args: &[String]) {
    // Parse flags first
    let mut cycles: u64 = 1;
    let mut path_arg: Option<&str> = None;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            cycles = args[i + 1].parse().unwrap_or(1);
            i += 2;
        } else if path_arg.is_none() {
            path_arg = Some(&args[i]);
            i += 1;
        } else {
            i += 1;
        }
    }

    let target = path_arg.map(Path::new);

    // Determine if single file or project
    let is_single_file = target
        .map(|p| p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), Some("st" | "scl")))
        .unwrap_or(false);

    let (parse_result, project_name, mut comm_setup) = if is_single_file {
        let path = target.unwrap();
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {e}", path.display());
                process::exit(1);
            }
        };

        let parse_result = parse_with_stdlib(&source);
        if !parse_result.errors.is_empty() {
            for err in &parse_result.errors {
                let (line, col) = byte_offset_to_line_col(&source, err.range.start);
                eprintln!("{}:{}:{}: error: {}", path.display(), line, col, err.message);
            }
            process::exit(1);
        }
        (parse_result, None, None)
    } else {
        // Project mode

        // Load comm config FIRST so the auto-generated `_io_map.st` is
        // present on disk before project autodiscovery walks the directory.
        // We need the project root for that — find it by walking up.
        let probe_root = resolve_project_root(target);
        let comm_setup = match comm_setup::load_for_project(&probe_root) {
            Ok(setup) => setup,
            Err(e) => {
                eprintln!("Comm config error: {e}");
                process::exit(1);
            }
        };
        if let Some(ref setup) = comm_setup {
            eprintln!(
                "[COMM] Generated I/O map: {} ({} device(s))",
                setup.io_map_path.display(),
                setup.config.devices.len()
            );
        }

        let project = match st_syntax::project::discover_project(target) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Project discovery error: {e}");
                process::exit(1);
            }
        };

        eprintln!("Project '{}': {} source file(s)", project.name, project.source_files.len());

        let parse_result = match parse_project(&project) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        };

        if !parse_result.errors.is_empty() {
            for err in &parse_result.errors {
                eprintln!("error: {}", err.message);
            }
            process::exit(1);
        }

        (parse_result, project.entry_point, comm_setup)
    };

    // Semantic check
    let analysis = st_semantics::analyze::analyze(&parse_result.source_file);
    let has_errors = analysis.diagnostics.iter().any(|d| {
        d.severity == st_semantics::diagnostic::Severity::Error
    });
    if has_errors {
        for d in &analysis.diagnostics {
            if d.severity == st_semantics::diagnostic::Severity::Error {
                eprintln!("error: {}", d.message);
            }
        }
        process::exit(1);
    }

    // Compile
    let module = match st_compiler::compile(&parse_result.source_file) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation error: {e}");
            process::exit(1);
        }
    };

    // Find the program to run
    let program_name = if let Some(entry) = project_name {
        // Use project-specified entry point
        if module.functions.iter().any(|f| f.name.eq_ignore_ascii_case(&entry)) {
            entry
        } else {
            eprintln!("Entry point PROGRAM '{entry}' not found");
            process::exit(1);
        }
    } else {
        // Use first PROGRAM found
        module
            .functions
            .iter()
            .find(|f| f.kind == st_ir::PouKind::Program)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| {
                eprintln!("No PROGRAM found");
                process::exit(1);
            })
    };

    // Load engine config (cycle_time, ...) from plc-project.yaml if present.
    let engine_proj = if is_single_file {
        st_comm_api::EngineProjectConfig::default()
    } else {
        comm_setup::load_engine_config(&resolve_project_root(target))
    };
    if let Some(ct) = engine_proj.cycle_time {
        eprintln!("[ENGINE] cycle_time: {ct:?}");
    }

    // Resolve retain path: <project-root>/.st-retain/<program>.retain
    let retain_config = if !is_single_file {
        let root = resolve_project_root(target);
        let retain_dir = root.join(".st-retain");
        let retain_path = retain_dir.join(format!("{program_name}.retain"));
        Some(st_engine::RetainConfig {
            path: retain_path,
            checkpoint_cycles: engine_proj.retain_checkpoint_cycles.unwrap_or(0),
        })
    } else {
        None
    };

    let config = st_engine::EngineConfig {
        max_cycles: cycles,
        cycle_time: engine_proj.cycle_time,
        retain: retain_config,
        ..Default::default()
    };
    let mut engine = st_engine::Engine::new(module, program_name, config);

    // Register simulated devices and start their web UIs (if any).
    if let Some(ref mut setup) = comm_setup {
        comm_setup::register_simulated_devices(setup, &mut engine);
        comm_setup::start_web_uis(setup, 8080);
    }

    let wall_started = std::time::Instant::now();
    match engine.run() {
        Ok(()) => {
            let wall_total = wall_started.elapsed();
            let stats = engine.stats();
            // `stats.total_time` only sums execution time per cycle — it does
            // NOT include the inter-cycle sleep enforced by `engine.cycle_time`.
            // Report wall-clock total separately so users with a cycle_time
            // configured see the period they actually configured.
            if engine_proj.cycle_time.is_some() {
                eprintln!(
                    "Executed {} cycle(s) in {:?} wall ({:?} cpu, avg {:?}/cycle exec, {} instructions)",
                    stats.cycle_count,
                    wall_total,
                    stats.total_time,
                    stats.avg_cycle_time(),
                    engine.vm().instruction_count(),
                );
            } else {
                eprintln!(
                    "Executed {} cycle(s) in {:?} (avg {:?}/cycle, {} instructions)",
                    stats.cycle_count,
                    stats.total_time,
                    stats.avg_cycle_time(),
                    engine.vm().instruction_count(),
                );
            }
        }
        Err(e) => {
            eprintln!("Runtime error: {e}");
        }
    }

    // Save retained variables on exit
    if let Err(e) = engine.save_retain() {
        eprintln!("[RETAIN] Save failed: {e}");
    }
}

fn run_compile_cmd(args: &[String]) {
    // Parse args: compile <path> -o <output>
    let mut source_path: Option<&str> = None;
    let mut output_path: Option<&str> = None;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            output_path = Some(&args[i + 1]);
            i += 2;
        } else if source_path.is_none() {
            source_path = Some(&args[i]);
            i += 1;
        } else {
            i += 1;
        }
    }

    let source_path = source_path.unwrap_or_else(|| {
        eprintln!("Usage: st-cli compile <file> -o <output>");
        process::exit(1);
    });
    let output_path = output_path.unwrap_or_else(|| {
        eprintln!("Usage: st-cli compile <file> -o <output>");
        process::exit(1);
    });

    let source = match std::fs::read_to_string(source_path) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error reading '{source_path}': {e}"); process::exit(1); }
    };

    let parse_result = parse_with_stdlib(&source);
    if !parse_result.errors.is_empty() {
        for err in &parse_result.errors {
            let (line, col) = byte_offset_to_line_col(&source, err.range.start);
            eprintln!("{source_path}:{line}:{col}: error: {}", err.message);
        }
        process::exit(1);
    }

    let analysis = st_semantics::analyze::analyze(&parse_result.source_file);
    let has_errors = analysis.diagnostics.iter().any(|d| d.severity == st_semantics::diagnostic::Severity::Error);
    if has_errors {
        for d in &analysis.diagnostics {
            if d.severity == st_semantics::diagnostic::Severity::Error {
                let (line, col) = byte_offset_to_line_col(&source, d.range.start);
                eprintln!("{source_path}:{line}:{col}: error: {}", d.message);
            }
        }
        process::exit(1);
    }

    let module = match st_compiler::compile(&parse_result.source_file) {
        Ok(m) => m,
        Err(e) => { eprintln!("Compilation error: {e}"); process::exit(1); }
    };

    // Serialize module to JSON
    let json = serde_json::to_string_pretty(&module).unwrap_or_else(|e| {
        eprintln!("Serialization error: {e}"); process::exit(1);
    });

    match std::fs::write(output_path, &json) {
        Ok(()) => eprintln!("Compiled to {output_path} ({} bytes)", json.len()),
        Err(e) => { eprintln!("Error writing '{output_path}': {e}"); process::exit(1); }
    }
}

fn run_fmt_cmd(args: &[String]) {
    let path = args.get(2).map(|s| s.as_str());
    let target = path.map(Path::new);

    let is_single_file = target
        .map(|p| p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), Some("st" | "scl")))
        .unwrap_or(false);

    let files = if is_single_file {
        vec![target.unwrap().to_path_buf()]
    } else {
        // Discover project files
        let project = match st_syntax::project::discover_project(target) {
            Ok(p) => p,
            Err(e) => { eprintln!("Project discovery error: {e}"); process::exit(1); }
        };
        project.source_files
    };

    let mut formatted_count = 0;
    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => { eprintln!("Error reading {}: {e}", file.display()); continue; }
        };

        let formatted = format_st(&source);
        if formatted != source {
            match std::fs::write(file, &formatted) {
                Ok(()) => {
                    eprintln!("Formatted: {}", file.display());
                    formatted_count += 1;
                }
                Err(e) => eprintln!("Error writing {}: {e}", file.display()),
            }
        }
    }

    if formatted_count == 0 {
        eprintln!("All {} file(s) already formatted", files.len());
    } else {
        eprintln!("Formatted {formatted_count} file(s)");
    }
}

/// Format ST source: normalize indentation.
fn format_st(source: &str) -> String {
    let indent_str = "    ";
    let mut result = String::new();
    let mut indent_level: i32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        let upper = trimmed.to_uppercase();

        // Decrease indent for closing/transition keywords
        if upper.starts_with("END_")
            || upper == "ELSE"
            || upper.starts_with("ELSIF")
            || upper.starts_with("UNTIL")
        {
            indent_level = (indent_level - 1).max(0);
        }

        for _ in 0..indent_level {
            result.push_str(indent_str);
        }
        result.push_str(trimmed);
        result.push('\n');

        // Increase indent for opening keywords
        if upper.starts_with("PROGRAM ")
            || upper.starts_with("FUNCTION ")
            || upper.starts_with("FUNCTION_BLOCK ")
            || upper.starts_with("VAR")
            || upper.starts_with("IF ") || upper == "ELSE"
            || upper.starts_with("ELSIF ")
            || upper.starts_with("FOR ")
            || upper.starts_with("WHILE ")
            || upper.starts_with("REPEAT")
            || upper.starts_with("CASE ")
            || upper.starts_with("STRUCT")
            || upper.starts_with("TYPE")
        {
            indent_level += 1;
        }
    }

    result
}

/// Print diagnostics in human-readable or JSON format. Returns true if any errors.
fn print_diagnostics(
    diagnostics: &[st_semantics::diagnostic::Diagnostic],
    source: &str,
    file_name: &str,
    json_output: bool,
) -> bool {
    let mut has_errors = false;

    if json_output {
        let items: Vec<serde_json::Value> = diagnostics.iter().map(|d| {
            let severity = match d.severity {
                st_semantics::diagnostic::Severity::Error => { has_errors = true; "error" }
                st_semantics::diagnostic::Severity::Warning => "warning",
                st_semantics::diagnostic::Severity::Info => "info",
            };
            let (line, col) = if !source.is_empty() {
                byte_offset_to_line_col(source, d.range.start)
            } else {
                (0, 0)
            };
            serde_json::json!({
                "file": file_name,
                "line": line,
                "column": col,
                "severity": severity,
                "code": format!("{:?}", d.code),
                "message": d.message
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
    } else {
        for d in diagnostics {
            let severity = match d.severity {
                st_semantics::diagnostic::Severity::Error => { has_errors = true; "error" }
                st_semantics::diagnostic::Severity::Warning => "warning",
                st_semantics::diagnostic::Severity::Info => "info",
            };
            if !source.is_empty() {
                let (line, col) = byte_offset_to_line_col(source, d.range.start);
                eprintln!("{file_name}:{line}:{col}: {severity}: {}", d.message);
            } else {
                eprintln!("{severity}: {}", d.message);
            }
        }
    }
    has_errors
}

/// Resolve the project root directory from an optional CLI path argument,
/// the same way `discover_project` would, but returning just the root.
fn resolve_project_root(target: Option<&Path>) -> std::path::PathBuf {
    let path = target.unwrap_or_else(|| Path::new("."));
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical.is_dir() {
        return canonical;
    }
    if canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "yaml" || e == "yml")
        .unwrap_or(false)
    {
        return canonical
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(canonical);
    }
    // Single .st file: walk up looking for plc-project.yaml
    let mut cur = canonical.clone();
    if cur.is_file() {
        if let Some(parent) = cur.parent() {
            cur = parent.to_path_buf();
        }
    }
    let mut probe = cur.clone();
    loop {
        if probe.join("plc-project.yaml").exists() || probe.join("plc-project.yml").exists() {
            return probe;
        }
        if !probe.pop() {
            break;
        }
    }
    cur
}

fn run_bundle_cmd(args: &[String]) {
    let mut path_arg: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut mode = st_deploy::BundleMode::Development;
    let mut inspect_path: Option<&str> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "inspect" if i == 2 => {
                inspect_path = args.get(3).map(|s| s.as_str());
                break;
            }
            "--release" => mode = st_deploy::BundleMode::Release,
            "--release-debug" => mode = st_deploy::BundleMode::ReleaseDebug,
            "-o" | "--output" if i + 1 < args.len() => {
                output = Some(&args[i + 1]);
                i += 1;
            }
            _ if path_arg.is_none() => path_arg = Some(&args[i]),
            _ => {}
        }
        i += 1;
    }

    // Handle `bundle inspect <path>`
    if let Some(bundle_path) = inspect_path {
        match st_deploy::bundle::inspect_bundle(Path::new(bundle_path)) {
            Ok(info) => {
                eprintln!("Bundle: {bundle_path}");
                eprintln!("  Name:     {}", info.manifest.name);
                eprintln!("  Version:  {}", info.manifest.version);
                eprintln!("  Mode:     {}", info.manifest.mode);
                eprintln!("  Compiled: {}", info.manifest.compiled_at);
                eprintln!("  Compiler: {}", info.manifest.compiler_version);
                if let Some(ref ep) = info.manifest.entry_point {
                    eprintln!("  Entry:    {ep}");
                }
                eprintln!("  Checksum: {} ({})",
                    &info.manifest.bytecode_checksum[..16],
                    if info.checksum_valid { "valid" } else { "INVALID" },
                );
                eprintln!("  Size:     {} bytes", info.archive_size);
                eprintln!();
                eprintln!("Files:");
                for (path, size) in &info.files {
                    eprintln!("  {:>8}  {path}", format_size(*size));
                }
            }
            Err(e) => {
                eprintln!("Error inspecting bundle: {e}");
                process::exit(1);
            }
        }
        return;
    }

    // Ensure no `inspect` without path
    if args.get(2).map(|s| s.as_str()) == Some("inspect") {
        eprintln!("Usage: st-cli bundle inspect <bundle-path>");
        process::exit(1);
    }

    // Build bundle
    let target = path_arg.map(Path::new);
    let root = resolve_project_root(target);

    // Bundle requires a project with plc-project.yaml — autodiscovering
    // an arbitrary directory tree picks up unrelated files and produces
    // confusing duplicate-declaration errors.
    if !root.join("plc-project.yaml").exists() && !root.join("plc-project.yml").exists() {
        eprintln!(
            "No plc-project.yaml found in {} (or any parent directory).\n\
             Run this command from inside a project directory, or specify the path:\n  \
             st-cli bundle <project-dir>",
            root.display()
        );
        process::exit(1);
    }

    // Regenerate I/O map before bundling
    if let Err(e) = comm_setup::load_for_project(&root) {
        eprintln!("Comm config error: {e}");
        process::exit(1);
    }

    let options = st_deploy::bundle::BundleOptions {
        mode,
        output: output.map(PathBuf::from),
    };

    eprintln!("Compiling project in {}...", root.display());
    let bundle = match st_deploy::bundle::create_bundle(&root, &options) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Bundle error: {e}");
            process::exit(1);
        }
    };

    let out_path = options.output.unwrap_or_else(|| {
        root.join(format!("{}.st-bundle", bundle.manifest.name))
    });

    match st_deploy::bundle::write_bundle(&bundle, &out_path) {
        Ok(size) => {
            eprintln!(
                "Created {} ({}, {}, {} bytes)",
                out_path.display(),
                bundle.manifest.mode,
                bundle.manifest.version,
                size,
            );
        }
        Err(e) => {
            eprintln!("Error writing bundle: {e}");
            process::exit(1);
        }
    }
}

fn run_target_cmd(args: &[String]) {
    let subcmd = args.get(2).map(|s| s.as_str()).unwrap_or("help");

    match subcmd {
        "list" => {
            let path_arg = args.get(3).map(|s| s.as_str()).map(Path::new);
            let root = resolve_project_root(path_arg);
            let yaml_path = root.join("plc-project.yaml");
            let yml_path = root.join("plc-project.yml");

            let yaml_file = if yaml_path.exists() {
                yaml_path
            } else if yml_path.exists() {
                yml_path
            } else {
                eprintln!("No plc-project.yaml found in {}", root.display());
                process::exit(1);
            };

            let yaml_text = match std::fs::read_to_string(&yaml_file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Cannot read {}: {e}", yaml_file.display());
                    process::exit(1);
                }
            };

            match st_deploy::TargetConfig::from_project_yaml(&yaml_text) {
                Ok(config) => {
                    if config.targets.is_empty() {
                        eprintln!("No targets configured in {}", yaml_file.display());
                        return;
                    }
                    eprintln!("Deployment targets ({}):", yaml_file.display());
                    for t in &config.targets {
                        let default_marker = if config.default_target.as_deref() == Some(&t.name) {
                            " (default)"
                        } else {
                            ""
                        };
                        eprintln!(
                            "  {:<20} {}@{}:{} ({}/{}){default_marker}",
                            t.name, t.user, t.host, t.agent_port, t.os, t.arch,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error parsing targets: {e}");
                    process::exit(1);
                }
            }
        }
        "install" => {
            run_target_install(&args[3..]);
        }
        "uninstall" => {
            run_target_uninstall(&args[3..]);
        }
        _ => {
            eprintln!("Usage: st-cli target <subcommand>");
            eprintln!();
            eprintln!("Subcommands:");
            eprintln!("  list [path]       List configured deployment targets");
            eprintln!("  install user@host Install PLC runtime on a target device");
            eprintln!("  uninstall user@host Remove PLC runtime from a target");
            process::exit(if subcmd == "help" { 0 } else { 1 });
        }
    }
}

fn run_target_install(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: st-cli target install user@host [options]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --key <path>        SSH private key");
        eprintln!("  --port <port>       SSH port (default: 22)");
        eprintln!("  --agent-port <port> Agent HTTP port (default: 4840)");
        eprintln!("  --name <name>       Agent name");
        eprintln!("  --upgrade           Upgrade existing installation");
        process::exit(1);
    }

    let user_at_host = &args[0];
    let mut target = match st_deploy::SshTarget::parse(user_at_host) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let mut options = st_deploy::InstallOptions::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--key" if i + 1 < args.len() => {
                target = target.with_key(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--port" if i + 1 < args.len() => {
                target = target.with_port(args[i + 1].parse().unwrap_or(22));
                i += 2;
            }
            "--agent-port" if i + 1 < args.len() => {
                options.agent_port = args[i + 1].parse().unwrap_or(4840);
                i += 2;
            }
            "--name" if i + 1 < args.len() => {
                options.agent_name = args[i + 1].clone();
                i += 2;
            }
            "--upgrade" => {
                options.upgrade = true;
                i += 1;
            }
            other => {
                eprintln!("Unknown option: {other}");
                process::exit(1);
            }
        }
    }

    // Find the static binary for the target architecture
    // First detect the target's arch, then find the binary
    eprintln!("Connecting to {}@{}...", target.user, target.host);

    // Test connection first to get a good error before detecting platform
    if let Err(e) = target.test_connection() {
        eprintln!("Error: {e}");
        process::exit(1);
    }

    let (_os, arch) = match target.detect_platform() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error detecting target platform: {e}");
            process::exit(1);
        }
    };

    let binary_path = match st_deploy::installer::find_static_binary(&arch) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    eprintln!("  Binary: {} ({})", binary_path.display(), arch);

    let mut progress = |msg: &str| eprintln!("  {msg}");

    match st_deploy::installer::install(&target, &binary_path, &options, &mut progress) {
        Ok(result) => {
            eprintln!();
            eprintln!("Target {}@{} is ready.", target.user, target.host);
            eprintln!("  OS:     {} {}", result.os, result.arch);
            eprintln!("  Agent:  port {}", result.agent_port);
            eprintln!("  DAP:    port {}", result.dap_port);
            eprintln!("  Version: {}", result.version);
            eprintln!();
            eprintln!("Add to your plc-project.yaml:");
            eprintln!("  targets:");
            eprintln!("    - name: my-plc");
            eprintln!("      host: {}", target.host);
            eprintln!("      user: {}", target.user);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

fn run_target_uninstall(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: st-cli target uninstall user@host [--purge]");
        process::exit(1);
    }

    let user_at_host = &args[0];
    let mut target = match st_deploy::SshTarget::parse(user_at_host) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let mut purge = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--purge" => {
                purge = true;
                i += 1;
            }
            "--key" if i + 1 < args.len() => {
                target = target.with_key(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--port" if i + 1 < args.len() => {
                target = target.with_port(args[i + 1].parse().unwrap_or(22));
                i += 2;
            }
            other => {
                eprintln!("Unknown option: {other}");
                process::exit(1);
            }
        }
    }

    let mut progress = |msg: &str| eprintln!("  {msg}");

    match st_deploy::installer::uninstall(&target, purge, &mut progress) {
        Ok(()) => {
            eprintln!("PLC runtime uninstalled from {}@{}", target.user, target.host);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
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
