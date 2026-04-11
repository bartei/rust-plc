//! Test using the ACTUAL oop_project files from disk.

use st_ir::*;
use st_engine::*;
use std::path::Path;

#[test]
fn run_actual_oop_project() {
    let project = st_syntax::project::discover_project(
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("playground/oop_project").as_path())
    ).unwrap();

    let sources = st_syntax::project::load_project_sources(&project).unwrap();
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all_sources: Vec<&str> = stdlib.to_vec();
    for (_path, content) in &sources {
        all_sources.push(content.as_str());
    }

    let parse_result = st_syntax::multi_file::parse_multi(&all_sources);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);

    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");

    // Dump all functions to see what got compiled
    eprintln!("=== Compiled functions ===");
    for (i, func) in module.functions.iter().enumerate() {
        eprintln!("[{i}] {} ({:?}) locals={:?}",
            func.name, func.kind,
            func.locals.slots.iter().map(|s| &s.name).collect::<Vec<_>>());
    }

    let program_name = project.entry_point.unwrap_or_else(|| {
        module.functions.iter().find(|f| f.kind == PouKind::Program).unwrap().name.clone()
    });

    let config = EngineConfig { max_cycles: 5, ..Default::default() };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");

    let g_cycle = engine.vm().get_global("g_cycle");
    let g_raw = engine.vm().get_global("g_raw_temp");
    let g_filtered = engine.vm().get_global("g_filtered_temp");
    let g_samples = engine.vm().get_global("g_sensor_samples");
    let g_ctrl = engine.vm().get_global("g_ctrl_output");
    let g_enabled = engine.vm().get_global("g_ctrl_enabled");
    let g_setpoint = engine.vm().get_global("g_setpoint");

    eprintln!("\n=== Results after 5 cycles ===");
    eprintln!("g_cycle = {g_cycle:?}");
    eprintln!("g_raw_temp = {g_raw:?}");
    eprintln!("g_filtered_temp = {g_filtered:?}");
    eprintln!("g_sensor_samples = {g_samples:?}");
    eprintln!("g_ctrl_output = {g_ctrl:?}");
    eprintln!("g_ctrl_enabled = {g_enabled:?}");
    eprintln!("g_setpoint = {g_setpoint:?}");

    // cycle should be 5
    assert_eq!(g_cycle, Some(&Value::Int(5)));
    // g_raw_temp should NOT be 0 (simTemp > 200 at cycle 5)
    assert_ne!(g_raw, Some(&Value::Int(0)), "g_raw_temp should not be 0");
}

#[test]
fn dump_main_source_map() {
    let project = st_syntax::project::discover_project(
        Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("playground/oop_project").as_path())
    ).unwrap();

    let sources = st_syntax::project::load_project_sources(&project).unwrap();
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all_sources: Vec<&str> = stdlib.to_vec();
    
    // Find main.st content and its index
    let mut main_source = "";
    for (_path, content) in &sources {
        all_sources.push(content.as_str());
    }
    for (path, content) in &sources {
        if path.ends_with("main.st") {
            main_source = content.as_str();
        }
    }
    
    let parse_result = st_syntax::multi_file::parse_multi(&all_sources);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();

    let main_func = module.functions.iter().find(|f| f.name == "Main").unwrap();
    eprintln!("Main source map ({} entries):", main_func.source_map.len());
    for (i, sm) in main_func.source_map.iter().enumerate().take(20) {
        let snippet = if sm.byte_offset > 0 && sm.byte_offset < main_source.len() {
            &main_source[sm.byte_offset..sm.byte_end.min(main_source.len())]
        } else {
            "<out of range>"
        };
        eprintln!("  [{i}] offset={}-{} → {:?}", sm.byte_offset, sm.byte_end, &snippet[..snippet.len().min(40)]);
    }
    
    // Check what line 60 (approximate first executable line) maps to
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(main_source.bytes().enumerate().filter_map(|(i, b)| if b == b'\n' { Some(i + 1) } else { None }))
        .collect();
    eprintln!("\nmain.st has {} lines", line_offsets.len());
    for line_num in [55, 60, 65, 70, 82, 83] {
        if let Some(&offset) = line_offsets.get(line_num - 1) {
            let end = line_offsets.get(line_num).copied().unwrap_or(main_source.len());
            let text = main_source[offset..end].trim();
            eprintln!("  line {line_num}: offset={offset}-{end} text={text:?}");
            // Check if any Main instruction falls in this range
            let hit = main_func.source_map.iter().any(|sm| sm.byte_offset >= offset && sm.byte_offset < end && sm.byte_offset > 0);
            eprintln!("    → breakpoint match: {hit}");
        }
    }
}
