use std::env;
use std::path::Path;
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
    eprintln!("  debug <file>      Start DAP debug server (stdin/stdout)");
    eprintln!("  help              Show this help message");
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
    let target = path.map(Path::new);

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

        let mut has_errors = false;
        for d in &result.diagnostics {
            let severity = match d.severity {
                st_semantics::diagnostic::Severity::Error => { has_errors = true; "error" }
                st_semantics::diagnostic::Severity::Warning => "warning",
                st_semantics::diagnostic::Severity::Info => "info",
            };
            let (line, col) = byte_offset_to_line_col(&source, d.range.start);
            eprintln!("{}:{}:{}: {}: {}", path.display(), line, col, severity, d.message);
        }

        if result.diagnostics.is_empty() {
            eprintln!("{}: OK", path.display());
        }
        if has_errors { process::exit(1); }
    } else {
        // Project mode
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
        let mut has_errors = false;
        for d in &result.diagnostics {
            if d.severity == st_semantics::diagnostic::Severity::Error {
                has_errors = true;
                eprintln!("error: {}", d.message);
            }
        }

        if !has_errors {
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

    let (parse_result, project_name) = if is_single_file {
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
        (parse_result, None)
    } else {
        // Project mode
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

        (parse_result, project.entry_point)
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

    // Run
    let config = st_runtime::EngineConfig {
        max_cycles: cycles,
        ..Default::default()
    };
    let mut engine = st_runtime::Engine::new(module, program_name, config);
    match engine.run() {
        Ok(()) => {
            let stats = engine.stats();
            eprintln!(
                "Executed {} cycle(s) in {:?} (avg {:?}/cycle, {} instructions)",
                stats.cycle_count,
                stats.total_time,
                stats.avg_cycle_time(),
                engine.vm().instruction_count(),
            );
        }
        Err(e) => {
            eprintln!("Runtime error: {e}");
            process::exit(1);
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
