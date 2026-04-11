//! End-to-end tests verifying bundle mode artifacts from the **receiver** side.
//!
//! These tests simulate what the target agent sees: a bundle arrives, it is
//! extracted, and the receiver inspects the contents to verify that only the
//! expected artifacts are present for each bundle mode.
//!
//! Every assertion is from the receiver's perspective — we never look at the
//! in-memory `ProgramBundle` struct, only at what comes out of `extract_bundle`
//! after writing to disk and reading back (the actual on-wire path).

use st_deploy::bundle::{
    create_bundle, extract_bundle, inspect_bundle, write_bundle, BundleMode, BundleOptions,
};
use st_deploy::debug_info::DebugMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Variable names that appear in the ST source — used to verify stripping.
const PROPRIETARY_NAMES: &[&str] = &[
    "secret_counter",
    "motor_speed_setpoint",
    "tank_pressure",
    "filling_active",
    "pump_duty_cycle",
];

/// Create a project with proprietary variable names for IP protection testing.
fn create_ip_test_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    fs::write(
        root.join("plc-project.yaml"),
        "name: IPTest\nversion: '1.0.0'\nentryPoint: Main\n",
    )
    .unwrap();

    fs::write(
        root.join("main.st"),
        concat!(
            "FUNCTION_BLOCK MotorController\n",
            "VAR\n",
            "    motor_speed_setpoint : REAL := 0.0;\n",
            "    pump_duty_cycle : INT := 0;\n",
            "END_VAR\n",
            "    pump_duty_cycle := pump_duty_cycle + 1;\n",
            "END_FUNCTION_BLOCK\n",
            "\n",
            "PROGRAM Main\n",
            "VAR\n",
            "    secret_counter : INT := 0;\n",
            "    tank_pressure : REAL := 0.0;\n",
            "    filling_active : BOOL := FALSE;\n",
            "    ctrl : MotorController;\n",
            "END_VAR\n",
            "    secret_counter := secret_counter + 1;\n",
            "    tank_pressure := 1.5;\n",
            "    filling_active := secret_counter > 10;\n",
            "    ctrl();\n",
            "END_PROGRAM\n",
        ),
    )
    .unwrap();

    (dir, root)
}

/// Create a bundle, write to disk, extract — simulating the full sender→receiver path.
fn bundle_roundtrip(root: &Path, mode: BundleMode) -> (st_deploy::ProgramBundle, PathBuf) {
    let options = BundleOptions {
        mode,
        ..Default::default()
    };

    let bundle = create_bundle(root, &options).unwrap();
    let bundle_path = root.join(format!("test-{mode}.st-bundle"));
    write_bundle(&bundle, &bundle_path).unwrap();

    // Extract from disk — this is what the agent does
    let extracted = extract_bundle(&bundle_path).unwrap();
    (extracted, bundle_path)
}

// ─── DEVELOPMENT MODE: receiver sees everything ────────────────────────

#[test]
fn receiver_development_has_source_files() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Development);

    assert_eq!(bundle.manifest.mode, BundleMode::Development);
    assert!(
        !bundle.sources.is_empty(),
        "Development bundle receiver must see source files"
    );

    // Source content should contain the original ST code
    let source_text: String = bundle
        .sources
        .iter()
        .map(|(_, content)| String::from_utf8_lossy(content).to_string())
        .collect();
    assert!(
        source_text.contains("secret_counter"),
        "Development source should contain original variable names"
    );
}

#[test]
fn receiver_development_has_full_debug_map() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Development);

    assert!(bundle.manifest.has_debug_map);
    let debug_map_bytes = bundle.debug_map.expect("Development bundle must have debug.map");
    let dm: DebugMap = serde_json::from_slice(&debug_map_bytes).unwrap();

    // Debug map should contain original variable names
    let all_local_names: Vec<&str> = dm
        .functions
        .iter()
        .flat_map(|f| f.local_names.iter().map(|n| n.as_str()))
        .collect();

    assert!(
        all_local_names.contains(&"secret_counter"),
        "Development debug.map should have original variable names: {all_local_names:?}"
    );
    assert!(
        all_local_names.contains(&"motor_speed_setpoint"),
        "Development debug.map should have FB variable names: {all_local_names:?}"
    );

    // Source maps should be populated
    for func in &dm.functions {
        assert!(
            !func.source_map.is_empty(),
            "Development debug.map function '{}' should have source maps",
            func.name
        );
    }
}

#[test]
fn receiver_development_bytecode_has_original_names() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Development);

    let bytecode_str = String::from_utf8_lossy(&bundle.bytecode);
    for name in PROPRIETARY_NAMES {
        assert!(
            bytecode_str.contains(name),
            "Development bytecode should contain '{name}'"
        );
    }
}

// ─── RELEASE MODE: receiver sees nothing proprietary ───────────────────

#[test]
fn receiver_release_has_no_source_files() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    assert_eq!(bundle.manifest.mode, BundleMode::Release);
    assert!(
        bundle.sources.is_empty(),
        "Release bundle receiver must NOT see source files"
    );
    assert!(bundle.manifest.source_files.is_empty());
}

#[test]
fn receiver_release_has_no_debug_map() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    assert!(!bundle.manifest.has_debug_map);
    assert!(
        bundle.debug_map.is_none(),
        "Release bundle receiver must NOT see debug.map"
    );
}

#[test]
fn receiver_release_bytecode_has_no_proprietary_names() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    let bytecode_str = String::from_utf8_lossy(&bundle.bytecode);
    for name in PROPRIETARY_NAMES {
        assert!(
            !bytecode_str.contains(name),
            "Release bytecode must NOT contain proprietary name '{name}'"
        );
    }
}

#[test]
fn receiver_release_bytecode_has_no_source_maps() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    // Deserialize the stripped module and verify source maps are empty
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    for func in &module.functions {
        assert!(
            func.source_map.is_empty(),
            "Release bytecode function '{}' must have empty source_map",
            func.name
        );
    }
}

#[test]
fn receiver_release_bytecode_has_opaque_var_names() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();

    // Global variable names should be opaque indices
    for slot in &module.globals.slots {
        assert!(
            slot.name.starts_with('g'),
            "Release global var should be 'gN', got: '{}'",
            slot.name
        );
    }

    // Local variable names should be opaque indices
    for (fi, func) in module.functions.iter().enumerate() {
        for slot in &func.locals.slots {
            let expected_prefix = format!("f{fi}_v");
            assert!(
                slot.name.starts_with(&expected_prefix),
                "Release local var should start with '{}', got: '{}'",
                expected_prefix,
                slot.name
            );
        }
    }
}

#[test]
fn receiver_release_bytecode_still_has_pou_names() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    // POU names are kept because the runtime needs them to find the entry point
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    assert!(
        module.find_function("Main").is_some(),
        "Release bytecode must keep POU name 'Main' for runtime entry point"
    );
}

#[test]
fn receiver_release_bytecode_is_still_executable() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    // The stripped module must still be a valid Module that could run
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    assert!(!module.functions.is_empty());

    // Instructions must still be present
    let main = module.find_function("Main").unwrap().1;
    assert!(
        !main.instructions.is_empty(),
        "Release bytecode must keep instructions"
    );
    assert!(main.register_count > 0);
}

#[test]
fn receiver_release_archive_has_no_source_or_debug_entries() {
    let (_dir, root) = create_ip_test_project();
    let (_, bundle_path) = bundle_roundtrip(&root, BundleMode::Release);

    let info = inspect_bundle(&bundle_path).unwrap();
    for (path, _) in &info.files {
        assert!(
            !path.starts_with("source/"),
            "Release archive must not contain source/ entries, found: {path}"
        );
        assert!(
            path != "debug.map",
            "Release archive must not contain debug.map"
        );
    }
}

// ─── RELEASE-DEBUG MODE: receiver sees obfuscated debug info ───────────

#[test]
fn receiver_release_debug_has_no_source_files() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::ReleaseDebug);

    assert_eq!(bundle.manifest.mode, BundleMode::ReleaseDebug);
    assert!(
        bundle.sources.is_empty(),
        "ReleaseDebug bundle receiver must NOT see source files"
    );
}

#[test]
fn receiver_release_debug_has_obfuscated_debug_map() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::ReleaseDebug);

    assert!(bundle.manifest.has_debug_map);
    let debug_map_bytes = bundle.debug_map.expect("ReleaseDebug must have debug.map");
    let dm: DebugMap = serde_json::from_slice(&debug_map_bytes).unwrap();

    // Variable names should be opaque indices, not original names
    let debug_map_json = String::from_utf8_lossy(&debug_map_bytes);
    for name in PROPRIETARY_NAMES {
        assert!(
            !debug_map_json.contains(name),
            "ReleaseDebug debug.map must NOT contain proprietary name '{name}'"
        );
    }

    // Local variable names should be obfuscated (v0, v1, ...)
    let all_local_names: Vec<&str> = dm
        .functions
        .iter()
        .flat_map(|f| f.local_names.iter().map(|n| n.as_str()))
        .collect();
    assert!(
        all_local_names.iter().all(|n| n.starts_with('v')),
        "ReleaseDebug debug.map local names should be obfuscated (v0, v1, ...), got: {all_local_names:?}"
    );

    // Global names (if any) should be obfuscated (g0, g1, ...)
    for gname in &dm.global_names {
        assert!(
            gname.starts_with('g'),
            "ReleaseDebug global name should be obfuscated, got: '{gname}'"
        );
    }

    // POU names should be kept (for stack traces)
    assert!(
        dm.functions.iter().any(|f| f.name == "Main"),
        "ReleaseDebug debug.map should keep POU name 'Main'"
    );

    // Source maps should be present (for line-based breakpoints)
    for func in &dm.functions {
        assert!(
            !func.source_map.is_empty(),
            "ReleaseDebug debug.map function '{}' should have source maps",
            func.name
        );
    }
}

#[test]
fn receiver_release_debug_bytecode_has_no_proprietary_names() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::ReleaseDebug);

    let bytecode_str = String::from_utf8_lossy(&bundle.bytecode);
    for name in PROPRIETARY_NAMES {
        assert!(
            !bytecode_str.contains(name),
            "ReleaseDebug bytecode must NOT contain proprietary name '{name}'"
        );
    }
}

#[test]
fn receiver_release_debug_bytecode_keeps_source_maps() {
    let (_dir, root) = create_ip_test_project();
    let (bundle, _) = bundle_roundtrip(&root, BundleMode::ReleaseDebug);

    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    let main = module.find_function("Main").unwrap().1;
    assert!(
        !main.source_map.is_empty(),
        "ReleaseDebug bytecode should keep source maps for line-based breakpoints"
    );
}

#[test]
fn receiver_release_debug_archive_has_debug_map_but_no_source() {
    let (_dir, root) = create_ip_test_project();
    let (_, bundle_path) = bundle_roundtrip(&root, BundleMode::ReleaseDebug);

    let info = inspect_bundle(&bundle_path).unwrap();
    let file_names: Vec<&str> = info.files.iter().map(|(p, _)| p.as_str()).collect();

    assert!(
        file_names.contains(&"debug.map"),
        "ReleaseDebug archive should contain debug.map"
    );
    assert!(
        !file_names.iter().any(|p| p.starts_with("source/")),
        "ReleaseDebug archive must not contain source/ entries"
    );
}

// ─── CROSS-MODE: same project, different bundles ───────────────────────

#[test]
fn receiver_all_modes_produce_valid_bytecode() {
    let (_dir, root) = create_ip_test_project();

    for mode in [
        BundleMode::Development,
        BundleMode::Release,
        BundleMode::ReleaseDebug,
    ] {
        let (bundle, _) = bundle_roundtrip(&root, mode);
        let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode)
            .unwrap_or_else(|e| panic!("{mode} bytecode should be valid Module JSON: {e}"));

        assert!(
            module.find_function("Main").is_some(),
            "{mode} bytecode must contain Main program"
        );
        assert!(
            !module.functions.is_empty(),
            "{mode} bytecode must have functions"
        );
    }
}

#[test]
fn receiver_release_bundle_is_smaller_than_development() {
    let (_dir, root) = create_ip_test_project();

    let (_, dev_path) = bundle_roundtrip(&root, BundleMode::Development);
    let (_, rel_path) = bundle_roundtrip(&root, BundleMode::Release);

    let dev_size = fs::metadata(&dev_path).unwrap().len();
    let rel_size = fs::metadata(&rel_path).unwrap().len();

    assert!(
        rel_size < dev_size,
        "Release bundle ({rel_size} bytes) should be smaller than development ({dev_size} bytes)"
    );
}

#[test]
fn receiver_development_and_release_bytecodes_differ() {
    let (_dir, root) = create_ip_test_project();

    let (dev_bundle, _) = bundle_roundtrip(&root, BundleMode::Development);
    let (rel_bundle, _) = bundle_roundtrip(&root, BundleMode::Release);

    assert_ne!(
        dev_bundle.bytecode, rel_bundle.bytecode,
        "Release bytecode should differ from development (names stripped)"
    );
}

// ─── AGENT-SIDE ENFORCEMENT: TODO markers ──────────────────────────────
//
// The following behaviors require the st-target-agent (Phase 15b) and cannot
// be tested until it exists. They are documented here as TODOs so they don't
// get forgotten.
//
// TODO(agent): Agent rejects DAP attach for release bundles (no debug info)
//   - POST /api/v1/program/upload with a release bundle
//   - WS connect to /ws/dap → agent should respond with an error:
//     "Debug not available: bundle was built in release mode"
//   - Verify agent returns 403 or similar
//
// TODO(agent): Agent allows DAP attach for development bundles (full debug)
//   - POST /api/v1/program/upload with a development bundle
//   - WS connect to /ws/dap → agent should proxy DAP normally
//   - Breakpoints, stepping, variable inspection all work
//
// TODO(agent): Agent allows DAP attach for release-debug bundles (limited debug)
//   - POST /api/v1/program/upload with a release-debug bundle
//   - WS connect to /ws/dap → agent should proxy DAP
//   - Line-based breakpoints work (source maps present)
//   - Variable names show as v0, v1, ... (obfuscated)
//   - Source text NOT available (no source/ in bundle)
//
// TODO(agent): Agent reports bundle mode in /api/v1/program/info
//   - GET /api/v1/program/info should return { "mode": "release" }
//   - VS Code can use this to warn: "Limited debug: release-debug mode"
//
// TODO(agent): Runtime skips debug hook setup for release bundles (performance)
//   - When bundle mode is release, the VM should not install breakpoint
//     hooks or source map lookups — pure execution speed.
//   - Measurable perf difference in cycle time benchmarks.
//
// These TODOs should be converted to real tests in Phase 15b (Target Agent Core)
// and Phase 15d (DAP & Monitor Proxy).
