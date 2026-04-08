//! Tests that reproduce the exact DAP breakpoint failure in multi-file projects.

use st_ir::*;
use st_runtime::debug::*;

/// Build the actual oop_project module and test breakpoint resolution.
#[test]
fn breakpoint_resolves_in_multi_file_project() {
    let project = st_syntax::project::discover_project(
        Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent().unwrap().parent().unwrap()
                .join("playground/oop_project")
                .as_path(),
        ),
    ).unwrap();

    let sources = st_syntax::project::load_project_sources(&project).unwrap();
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all_sources: Vec<&str> = stdlib.iter().copied().collect();

    let mut main_source = String::new();
    let mut main_path = String::new();
    for (path, content) in &sources {
        all_sources.push(content.as_str());
        if path.ends_with("main.st") {
            main_source = content.clone();
            main_path = path.to_string_lossy().to_string();
        }
    }
    assert!(!main_source.is_empty(), "main.st not found in project");

    let parse_result = st_syntax::multi_file::parse_multi(&all_sources);
    assert!(parse_result.errors.is_empty());
    let module = st_compiler::compile(&parse_result.source_file).unwrap();

    // Find the Main function
    let main_func = module.functions.iter()
        .find(|f| f.name == "Main")
        .expect("Main function not found");

    eprintln!("main.st is {} bytes", main_source.len());
    eprintln!("Main function has {} source_map entries", main_func.source_map.len());

    // Show first 10 source_map entries for Main
    for (i, sm) in main_func.source_map.iter().enumerate().take(10) {
        let snippet = if sm.byte_offset > 0 && sm.byte_offset < main_source.len()
            && sm.byte_end <= main_source.len()
        {
            &main_source[sm.byte_offset..sm.byte_end.min(main_source.len())]
        } else {
            "<out of range>"
        };
        eprintln!("  Main sm[{i}]: {}-{} {:?}", sm.byte_offset, sm.byte_end,
            &snippet[..snippet.len().min(50)]);
    }

    // Find the byte offset for a specific line in main.st
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(main_source.bytes().enumerate()
            .filter_map(|(i, b)| if b == b'\n' { Some(i + 1) } else { None }))
        .collect();

    // Find the first executable line (cycle := cycle + 1)
    let target_line = line_offsets.iter().enumerate()
        .find(|&(_, offset)| {
            let end = main_source[*offset..].find('\n').map(|n| *offset + n).unwrap_or(main_source.len());
            let text = main_source[*offset..end].trim();
            text == "cycle := cycle + 1;"
        })
        .map(|(i, _)| i + 1)  // 1-indexed
        .expect("Could not find 'cycle := cycle + 1;' in main.st");

    eprintln!("\nTarget: line {target_line} ('cycle := cycle + 1;')");

    // Set a breakpoint on that line
    let mut debug = DebugState::new();
    let results = debug.set_line_breakpoints(&module, &main_source, &[target_line as u32]);
    eprintln!("Breakpoint result: {results:?}");
    assert!(results[0].is_some(), "Breakpoint on line {target_line} should resolve");

    let bp_offset = results[0].unwrap();
    eprintln!("Breakpoint set at byte offset {bp_offset}");

    // Now verify: when we walk through Main's source_map, do we find this offset?
    let found_in_main = main_func.source_map.iter().enumerate()
        .find(|(_, sm)| sm.byte_offset == bp_offset);
    eprintln!("Found in Main source_map: {found_in_main:?}");
    assert!(found_in_main.is_some(),
        "Breakpoint offset {bp_offset} must exist in Main's source_map for check_breakpoint to trigger");

    // Also verify: the byte offset is from main.st (not some other file)
    assert!(bp_offset < main_source.len(),
        "Breakpoint offset {bp_offset} should be within main.st ({} bytes)", main_source.len());

    // Verify the source text at that offset makes sense
    if bp_offset < main_source.len() {
        let end = main_source[bp_offset..].find('\n').map(|n| bp_offset + n).unwrap_or(main_source.len());
        let text = &main_source[bp_offset..end];
        eprintln!("Source at breakpoint: {:?}", text.trim());
    }
}

/// End-to-end test: set breakpoint, run VM in Continue mode, verify it halts.
#[test]
fn breakpoint_actually_halts_vm_single_file() {
    let source = r#"
VAR_GLOBAL g_x : INT; END_VAR
PROGRAM Main
VAR x : INT := 0; END_VAR
    x := x + 1;
    g_x := x;
END_PROGRAM
"#;
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();

    let mut vm = st_runtime::vm::Vm::new(module.clone(), st_runtime::vm::VmConfig::default());

    // Set breakpoint on line 5 ("x := x + 1;")
    let results = vm.debug_mut().set_line_breakpoints(&module, source, &[5]);
    eprintln!("BP results: {results:?}");
    assert!(results[0].is_some(), "Breakpoint should resolve");

    // Run in Continue mode
    vm.debug_mut().resume(StepMode::Continue, 0);
    let result = vm.run("Main");
    eprintln!("Run result: {result:?}");

    // It should halt on the breakpoint
    assert!(result.is_err(), "VM should halt on breakpoint");
    if let Err(st_runtime::vm::VmError::Halt) = result {
        assert_eq!(vm.debug_state().pause_reason, PauseReason::Breakpoint,
            "Should be a breakpoint halt, got {:?}", vm.debug_state().pause_reason);
    } else {
        panic!("Expected Halt error, got {result:?}");
    }
}

/// End-to-end test: multi-file project with breakpoint.
#[test]
fn breakpoint_actually_halts_vm_multi_file() {
    let file_class = r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR c : Counter; x : INT := 0; END_VAR
    x := x + 1;
    c.Inc();
    g_val := c.Get();
END_PROGRAM
"#;
    let parse_result = st_syntax::multi_file::parse_multi(&[file_class, file_main]);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();

    let mut vm = st_runtime::vm::Vm::new(module.clone(), st_runtime::vm::VmConfig::default());

    // Set breakpoint on line 5 of file_main ("x := x + 1;")
    let results = vm.debug_mut().set_line_breakpoints(&module, file_main, &[5]);
    eprintln!("Multi-file BP results: {results:?}");
    assert!(results[0].is_some(), "Breakpoint should resolve");

    // Run in Continue mode
    vm.debug_mut().resume(StepMode::Continue, 0);
    let result = vm.run("Main");
    eprintln!("Multi-file run result: {result:?}");

    assert!(matches!(result, Err(st_runtime::vm::VmError::Halt)),
        "VM should halt on breakpoint, got {result:?}");
    assert_eq!(vm.debug_state().pause_reason, PauseReason::Breakpoint);
}

/// Test that breakpoint resolution doesn't pick up wrong-file functions.
#[test]
fn breakpoint_does_not_match_wrong_file() {
    // Simulate: two files with overlapping byte offset ranges
    let file_a = r#"
CLASS Small
METHOD Foo
END_METHOD
END_CLASS
"#;
    let file_b = r#"
VAR_GLOBAL g : INT; END_VAR
PROGRAM Main
VAR x : INT := 0; END_VAR
    x := x + 1;
    g := x;
END_PROGRAM
"#;
    let parse_result = st_syntax::multi_file::parse_multi(&[file_a, file_b]);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();

    // Set breakpoint on the executable line in file_b ("x := x + 1;")
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(file_b.bytes().enumerate()
            .filter_map(|(i, b)| if b == b'\n' { Some(i + 1) } else { None }))
        .collect();

    // Find the line with "x := x + 1;"
    let target_line = line_offsets.iter().enumerate()
        .find(|&(_, offset)| {
            let end = file_b[*offset..].find('\n').map(|n| *offset + n).unwrap_or(file_b.len());
            file_b[*offset..end].trim() == "x := x + 1;"
        })
        .map(|(i, _)| i + 1)
        .expect("Could not find target line");

    eprintln!("file_b target line: {target_line}");

    let mut debug = DebugState::new();
    let results = debug.set_line_breakpoints(&module, file_b, &[target_line as u32]);
    eprintln!("Results: {results:?}");
    assert!(results[0].is_some(), "Should resolve breakpoint");

    let bp_offset = results[0].unwrap();

    // The breakpoint offset MUST be found in Main's source_map (not Small's)
    let main_func = module.functions.iter().find(|f| f.name == "Main").unwrap();
    let in_main = main_func.source_map.iter().any(|sm| sm.byte_offset == bp_offset);
    assert!(in_main, "Breakpoint offset must come from Main, not from Small class");
}