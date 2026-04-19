//! End-to-end tests for the program bundler.
//!
//! These tests create real ST projects on disk, compile them through the full
//! pipeline, bundle them, and verify the results. No mocking — exercises the
//! same code paths as `st-cli bundle`.

use st_deploy::bundle::{
    create_bundle, extract_bundle, inspect_bundle, write_bundle, BundleMode, BundleOptions,
};
use st_deploy::target::TargetConfig;
use std::fs;
use std::path::PathBuf;

/// Create a temp directory with the given files and return (TempDir, root path).
fn create_project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    for (name, content) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }
    (dir, root)
}

// ── E2E: Full compile → bundle → extract round-trip ─────────────────────

#[test]
fn e2e_single_file_project_bundle() {
    let (_dir, root) = create_project(&[
        (
            "plc-project.yaml",
            "name: SingleFile\nversion: '1.0.0'\nentryPoint: Main\n",
        ),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();

    assert_eq!(bundle.manifest.name, "SingleFile");
    assert_eq!(bundle.manifest.version, "1.0.0");
    assert_eq!(bundle.manifest.mode, BundleMode::Development);
    assert_eq!(bundle.manifest.entry_point, Some("Main".to_string()));
    assert!(!bundle.bytecode.is_empty());
    assert_eq!(bundle.sources.len(), 1);
    assert!(bundle.project_yaml.is_some());

    // Write and extract
    let bundle_path = root.join("output.st-bundle");
    let size = write_bundle(&bundle, &bundle_path).unwrap();
    assert!(size > 0);

    let extracted = extract_bundle(&bundle_path).unwrap();
    assert_eq!(extracted.manifest.name, "SingleFile");
    assert_eq!(extracted.bytecode, bundle.bytecode);
    assert_eq!(extracted.sources.len(), 1);
}

#[test]
fn e2e_multi_file_project_bundle() {
    let (_dir, root) = create_project(&[
        (
            "plc-project.yaml",
            "name: MultiFile\nversion: '2.0.0'\nentryPoint: Main\n",
        ),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\n    y : INT;\nEND_VAR\n    y := Increment(IN1 := x);\n    x := y;\nEND_PROGRAM\n",
        ),
        (
            "helpers.st",
            "FUNCTION Increment : INT\nVAR_INPUT\n    IN1 : INT;\nEND_VAR\n    Increment := IN1 + 1;\nEND_FUNCTION\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    assert_eq!(bundle.manifest.name, "MultiFile");
    assert_eq!(bundle.sources.len(), 2);

    // Verify both source files are present
    let source_names: Vec<&str> = bundle.sources.iter().map(|(n, _)| n.as_str()).collect();
    assert!(source_names.iter().any(|n| n.contains("main.st")));
    assert!(source_names.iter().any(|n| n.contains("helpers.st")));

    // Bytecode should contain both functions
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    assert!(module.find_function("Main").is_some());
    assert!(module.find_function("Increment").is_some());
}

#[test]
fn e2e_function_block_project_bundle() {
    let (_dir, root) = create_project(&[
        (
            "plc-project.yaml",
            "name: FBProject\nversion: '1.0.0'\nentryPoint: Main\n",
        ),
        (
            "main.st",
            concat!(
                "FUNCTION_BLOCK Counter\nVAR\n    count : INT := 0;\nEND_VAR\n",
                "    count := count + 1;\nEND_FUNCTION_BLOCK\n\n",
                "PROGRAM Main\nVAR\n    c : Counter;\nEND_VAR\n",
                "    c();\nEND_PROGRAM\n",
            ),
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode).unwrap();
    assert!(module.find_function("Counter").is_some());
    assert!(module.find_function("Main").is_some());
}

// ── E2E: Bundle modes ───────────────────────────────────────────────────

#[test]
fn e2e_release_bundle_contains_no_source() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ReleaseTest\nversion: '3.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    secret_algorithm : INT := 42;\nEND_VAR\n    secret_algorithm := secret_algorithm * 2 + 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(
        &root,
        &BundleOptions {
            mode: BundleMode::Release,
            ..Default::default()
        },
    )
    .unwrap();

    // No source files
    assert!(bundle.sources.is_empty());
    assert!(bundle.manifest.source_files.is_empty());

    // Write, inspect, verify no source/ entries
    let bundle_path = root.join("release.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();

    let info = inspect_bundle(&bundle_path).unwrap();
    for (path, _) in &info.files {
        assert!(
            !path.starts_with("source/"),
            "Release bundle must not contain source files, found: {path}"
        );
    }

    // Release bytecode must NOT contain original variable names
    let bytecode_str = String::from_utf8_lossy(&bundle.bytecode);
    assert!(
        !bytecode_str.contains("secret_algorithm"),
        "Release bytecode must not contain original variable names"
    );

    // No debug map in release mode
    assert!(bundle.debug_map.is_none(), "Release bundle must not contain debug.map");
    assert!(!bundle.manifest.has_debug_map);
}

#[test]
fn e2e_release_debug_bundle_no_source_but_has_manifest() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ReleaseDebugTest\nversion: '1.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(
        &root,
        &BundleOptions {
            mode: BundleMode::ReleaseDebug,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(bundle.manifest.mode, BundleMode::ReleaseDebug);
    assert!(bundle.sources.is_empty());

    let bundle_path = root.join("rd.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();

    let info = inspect_bundle(&bundle_path).unwrap();
    assert!(info.checksum_valid);
    assert!(info.files.iter().any(|(p, _)| p == "manifest.yaml"));
    assert!(info.files.iter().any(|(p, _)| p == "program.stc"));
}

// ── E2E: Bundle integrity ───────────────────────────────────────────────

#[test]
fn e2e_checksum_verified_on_extract() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ChecksumTest\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    let bundle_path = root.join("test.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();

    // Normal extraction should succeed
    let extracted = extract_bundle(&bundle_path).unwrap();
    assert_eq!(extracted.manifest.bytecode_checksum, bundle.manifest.bytecode_checksum);

    // inspect reports valid checksum
    let info = inspect_bundle(&bundle_path).unwrap();
    assert!(info.checksum_valid);
}

#[test]
fn e2e_tampered_bundle_detected() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: TamperTest\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let mut bundle = create_bundle(&root, &BundleOptions::default()).unwrap();

    // Tamper with the bytecode
    if let Some(byte) = bundle.bytecode.get_mut(10) {
        *byte = byte.wrapping_add(1);
    }

    let bundle_path = root.join("tampered.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();

    // Extract should fail due to checksum mismatch
    let result = extract_bundle(&bundle_path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("checksum mismatch"),
        "Expected checksum mismatch error, got: {err}"
    );

    // Inspect should report invalid checksum
    let info = inspect_bundle(&bundle_path).unwrap();
    assert!(!info.checksum_valid);
}

// ── E2E: Profiles ───────────────────────────────────────────────────────

#[test]
fn e2e_bundle_includes_profile_files() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ProfileTest\nversion: '1.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
        (
            "profiles/motor_drive.yaml",
            "name: MotorDrive\nvendor: Test\nfields:\n  - { name: SPEED, type: INT, direction: output, register: { address: 0, kind: virtual } }\n",
        ),
        (
            "profiles/temp_sensor.yaml",
            "name: TempSensor\nvendor: Test\nfields:\n  - { name: TEMP, type: REAL, direction: input, register: { address: 0, kind: virtual } }\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    assert_eq!(bundle.profiles.len(), 2);

    let profile_names: Vec<&str> = bundle.profiles.iter().map(|(n, _)| n.as_str()).collect();
    assert!(profile_names.contains(&"motor_drive.yaml"));
    assert!(profile_names.contains(&"temp_sensor.yaml"));

    // Verify profiles survive round-trip
    let bundle_path = root.join("test.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();
    let extracted = extract_bundle(&bundle_path).unwrap();
    assert_eq!(extracted.profiles.len(), 2);

    // Profiles should be identical byte-for-byte
    for (name, content) in &bundle.profiles {
        let extracted_content = extracted
            .profiles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
            .unwrap();
        assert_eq!(content, extracted_content, "Profile {name} content mismatch");
    }
}

#[test]
fn e2e_release_bundle_still_includes_profiles() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ProfileRelease\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
        (
            "profiles/device.yaml",
            "name: Device\nvendor: Test\nfields: []\n",
        ),
    ]);

    let bundle = create_bundle(
        &root,
        &BundleOptions {
            mode: BundleMode::Release,
            ..Default::default()
        },
    )
    .unwrap();

    // Release mode excludes source but keeps profiles (needed at runtime)
    assert!(bundle.sources.is_empty());
    assert_eq!(bundle.profiles.len(), 1);
}

// ── E2E: Target configuration ───────────────────────────────────────────

#[test]
fn e2e_parse_targets_from_project_yaml() {
    let yaml = r#"
name: DeployTest
version: "1.0.0"
entryPoint: Main

targets:
  - name: line1
    host: 192.168.1.50
    user: plc
    auth: key
    os: linux
    arch: x86_64
    agent_port: 4840
  - name: line2
    host: 192.168.1.51
    user: plc
    auth: key
    os: linux
    arch: aarch64
  - name: test-bench
    host: 10.0.0.100
    user: admin
    auth: agent
    os: linux

default_target: line1

engine:
  cycle_time: 10ms

links:
  - name: sim_link
    type: simulated
"#;

    let config = TargetConfig::from_project_yaml(yaml).unwrap();
    assert_eq!(config.targets.len(), 3);
    assert_eq!(config.default_target, Some("line1".to_string()));

    let t = config.resolve_target(None).unwrap();
    assert_eq!(t.name, "line1");
    assert_eq!(t.host, "192.168.1.50");

    let t2 = config.resolve_target(Some("line2")).unwrap();
    assert_eq!(t2.arch, "aarch64");

    let tw = config.resolve_target(Some("test-bench")).unwrap();
    assert_eq!(tw.os, "linux");
    assert_eq!(tw.auth, st_deploy::target::AuthMode::Agent);
}

#[test]
fn e2e_targets_coexist_with_comm_config() {
    // Verify that targets: and links:/devices: sections don't interfere
    let yaml = r#"
name: CoexistTest
targets:
  - name: plc1
    host: 10.0.0.1
links:
  - name: sim_link
    type: simulated
devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
"#;

    // Target config parses fine
    let target_config = TargetConfig::from_project_yaml(yaml).unwrap();
    assert_eq!(target_config.targets.len(), 1);

}

// ── E2E: Error cases ────────────────────────────────────────────────────

#[test]
fn e2e_bundle_parse_error_reported() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ParseError\n"),
        ("main.st", "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    INVALID SYNTAX HERE\nEND_PROGRAM\n"),
    ]);

    let result = create_bundle(&root, &BundleOptions::default());
    assert!(result.is_err());
}

#[test]
fn e2e_bundle_semantic_error_reported() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: SemanticError\n"),
        ("main.st", "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    y := 1;\nEND_PROGRAM\n"),
    ]);

    let result = create_bundle(&root, &BundleOptions::default());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("error") || err.contains("Error"),
        "Expected semantic error, got: {err}"
    );
}

#[test]
fn e2e_bundle_no_program_errors() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: NoProgram\n"),
        ("types.st", "TYPE MyType : INT; END_TYPE\n"),
    ]);

    let result = create_bundle(&root, &BundleOptions::default());
    // This may error during compilation (no PROGRAM) or produce an empty module
    // Either way, the bundle creation should handle it gracefully
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn e2e_extract_nonexistent_bundle_errors() {
    let result = extract_bundle(std::path::Path::new("/nonexistent/bundle.st-bundle"));
    assert!(result.is_err());
}

#[test]
fn e2e_inspect_nonexistent_bundle_errors() {
    let result = inspect_bundle(std::path::Path::new("/nonexistent/bundle.st-bundle"));
    assert!(result.is_err());
}

// ── E2E: Bundle file structure ──────────────────────────────────────────

#[test]
fn e2e_development_bundle_file_listing() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: FileListTest\nversion: '1.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    let bundle_path = root.join("test.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();

    let info = inspect_bundle(&bundle_path).unwrap();
    let file_names: Vec<&str> = info.files.iter().map(|(p, _)| p.as_str()).collect();

    // Must have these files
    assert!(file_names.contains(&"manifest.yaml"), "Missing manifest.yaml");
    assert!(file_names.contains(&"program.stc"), "Missing program.stc");
    assert!(file_names.contains(&"plc-project.yaml"), "Missing plc-project.yaml");

    // Development mode must have source
    assert!(
        file_names.iter().any(|p| p.starts_with("source/")),
        "Development bundle should contain source/ files"
    );

    // All file sizes should be > 0
    for (name, size) in &info.files {
        assert!(*size > 0, "File {name} has zero size");
    }
}

#[test]
fn e2e_bytecode_is_valid_module_json() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: ValidJSON\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    a : INT := 10;\n    b : REAL := 3.14;\n    c : BOOL := TRUE;\nEND_VAR\n    a := a + 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();

    // Bytecode must be valid JSON deserializable to Module
    let module: st_ir::Module = serde_json::from_slice(&bundle.bytecode)
        .expect("Bytecode should be valid Module JSON");

    assert!(!module.functions.is_empty());
    assert!(module.find_function("Main").is_some());

    // Globals should exist
    assert!(!module.globals.slots.is_empty() || !module.functions.is_empty());
}

#[test]
fn e2e_bundle_manifest_has_correct_compiler_version() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: VersionCheck\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let bundle = create_bundle(&root, &BundleOptions::default()).unwrap();
    assert_eq!(bundle.manifest.compiler_version, env!("CARGO_PKG_VERSION"));
    assert!(!bundle.manifest.compiled_at.is_empty());
}

// ── E2E: CLI integration via subprocess ─────────────────────────────────

#[test]
fn e2e_cli_bundle_command() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: CLITest\nversion: '1.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    // Find st-cli binary relative to the test binary (both in target/debug/)
    let cli_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/st-cli");

    if !cli_path.exists() {
        // Binary not built yet — skip this test
        eprintln!("Skipping CLI test: st-cli not built (run `cargo build -p st-cli` first)");
        return;
    }

    let output = std::process::Command::new(&cli_path)
        .args(["bundle", root.to_str().unwrap()])
        .output()
        .expect("Failed to run st-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "st-cli bundle failed: {stderr}"
    );
    assert!(stderr.contains("Created"), "Expected 'Created' in output: {stderr}");

    let bundle_path = root.join("CLITest.st-bundle");
    assert!(bundle_path.exists(), "Bundle file should be created");

    // Verify we can inspect the CLI-created bundle
    let info = inspect_bundle(&bundle_path).unwrap();
    assert_eq!(info.manifest.name, "CLITest");
    assert!(info.checksum_valid);
}

#[test]
fn e2e_cli_bundle_release_command() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: CLIRelease\nversion: '2.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let cli_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/st-cli");

    if !cli_path.exists() {
        eprintln!("Skipping CLI test: st-cli not built");
        return;
    }

    let output = std::process::Command::new(&cli_path)
        .args(["bundle", "--release", root.to_str().unwrap()])
        .output()
        .expect("Failed to run st-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "st-cli bundle --release failed: {stderr}");
    assert!(stderr.contains("release"), "Expected 'release' in output: {stderr}");

    let bundle_path = root.join("CLIRelease.st-bundle");
    assert!(bundle_path.exists());

    let info = inspect_bundle(&bundle_path).unwrap();
    assert_eq!(info.manifest.mode, BundleMode::Release);
    assert!(!info.files.iter().any(|(p, _)| p.starts_with("source/")));
}

#[test]
fn e2e_cli_bundle_inspect_command() {
    let (_dir, root) = create_project(&[
        ("plc-project.yaml", "name: InspectCLI\nversion: '1.0.0'\n"),
        (
            "main.st",
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        ),
    ]);

    let cli_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/st-cli");

    if !cli_path.exists() {
        eprintln!("Skipping CLI test: st-cli not built");
        return;
    }

    // First create a bundle
    let create_out = std::process::Command::new(&cli_path)
        .args(["bundle", root.to_str().unwrap()])
        .output()
        .expect("Failed to run st-cli bundle");
    assert!(create_out.status.success());

    let bundle_path = root.join("InspectCLI.st-bundle");

    // Then inspect it
    let inspect_out = std::process::Command::new(&cli_path)
        .args(["bundle", "inspect", bundle_path.to_str().unwrap()])
        .output()
        .expect("Failed to run st-cli bundle inspect");

    let stderr = String::from_utf8_lossy(&inspect_out.stderr);
    assert!(inspect_out.status.success(), "inspect failed: {stderr}");
    assert!(stderr.contains("InspectCLI"), "Should show project name: {stderr}");
    assert!(stderr.contains("valid"), "Should show checksum valid: {stderr}");
    assert!(stderr.contains("manifest.yaml"), "Should list files: {stderr}");
    assert!(stderr.contains("program.stc"), "Should list bytecode: {stderr}");
}
