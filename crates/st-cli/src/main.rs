use std::env;
use std::process;

fn print_usage() {
    eprintln!("st-cli: IEC 61131-3 Structured Text toolchain");
    eprintln!();
    eprintln!("Usage: st-cli <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  serve [--stdio]   Start the LSP server (default: stdio)");
    eprintln!("  check <file>      Parse and analyze a file, report diagnostics");
    eprintln!("  run <file> [-n N] Compile and execute a program (N cycles, default 1)");
    eprintln!("  debug <file>      Start DAP debug server (stdin/stdout)");
    eprintln!("  help              Show this help message");
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
            if args.len() < 3 {
                eprintln!("Usage: st-cli check <file>");
                process::exit(1);
            }
            let path = &args[2];
            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading '{path}': {e}");
                    process::exit(1);
                }
            };

            let result = st_semantics::check(&source);
            let mut has_errors = false;
            for d in &result.diagnostics {
                let severity = match d.severity {
                    st_semantics::diagnostic::Severity::Error => {
                        has_errors = true;
                        "error"
                    }
                    st_semantics::diagnostic::Severity::Warning => "warning",
                    st_semantics::diagnostic::Severity::Info => "info",
                };
                // Convert byte offset to line:col
                let (line, col) = byte_offset_to_line_col(&source, d.range.start);
                eprintln!("{}:{}:{}: {}: {}", path, line, col, severity, d.message);
            }

            if result.diagnostics.is_empty() {
                eprintln!("{path}: OK");
            }

            if has_errors {
                process::exit(1);
            }
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: st-cli run <file> [-n <cycles>]");
                process::exit(1);
            }
            let path = &args[2];
            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading '{path}': {e}");
                    process::exit(1);
                }
            };

            // Parse number of cycles from -n flag
            let mut cycles: u64 = 1;
            let mut i = 3;
            while i < args.len() {
                if args[i] == "-n" && i + 1 < args.len() {
                    cycles = args[i + 1].parse().unwrap_or(1);
                    i += 2;
                } else {
                    i += 1;
                }
            }

            // Parse
            let parse_result = st_syntax::parse(&source);
            if !parse_result.errors.is_empty() {
                for err in &parse_result.errors {
                    let (line, col) = byte_offset_to_line_col(&source, err.range.start);
                    eprintln!("{}:{}:{}: error: {}", path, line, col, err.message);
                }
                process::exit(1);
            }

            // Semantic check
            let analysis = st_semantics::analyze::analyze(&parse_result.source_file);
            let has_errors = analysis.diagnostics.iter().any(|d| {
                d.severity == st_semantics::diagnostic::Severity::Error
            });
            if has_errors {
                for d in &analysis.diagnostics {
                    if d.severity == st_semantics::diagnostic::Severity::Error {
                        let (line, col) = byte_offset_to_line_col(&source, d.range.start);
                        eprintln!("{}:{}:{}: error: {}", path, line, col, d.message);
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

            // Find the program to run (first PROGRAM POU)
            let program_name = module
                .functions
                .iter()
                .find(|f| f.kind == st_ir::PouKind::Program)
                .map(|f| f.name.clone())
                .unwrap_or_else(|| {
                    eprintln!("No PROGRAM found in '{path}'");
                    process::exit(1);
                });

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

fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut col = 1;
    for (i, b) in source.bytes().enumerate() {
        if i >= offset {
            break;
        }
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
