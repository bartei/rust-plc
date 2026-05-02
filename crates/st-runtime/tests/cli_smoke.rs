//! Subprocess-driven smoke tests for the `st-runtime` binary.
//!
//! `crates/st-runtime/src/main.rs` is the unified static binary deployed to
//! target devices (subcommands: `agent`, `debug`, `run`, `check`, `version`).
//! It sits at 0% acceptance coverage today because the only existing
//! integration test (`tests/e2e_installer.rs`) is QEMU-gated, so plain
//! `cargo test` builds the binary but never runs it. These tests fix that
//! by spawning the binary directly for the fast subcommands.
//!
//! The `agent` subcommand is intentionally NOT tested here — it binds a
//! port and runs forever, which is what `st-target-agent`'s `cli_smoke`
//! and `api_integration` already cover.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn st_runtime() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_st-runtime") {
        return PathBuf::from(p);
    }
    let test_exe = std::env::current_exe().expect("current_exe");
    let dir = test_exe.parent().and_then(|p| p.parent()).expect("target dir");
    dir.join("st-runtime")
}

fn run(args: &[&str]) -> Output {
    Command::new(st_runtime())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("st-runtime failed to launch")
}

fn write(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

const VALID_PROGRAM: &str = "\
PROGRAM Main\n\
VAR\n\
    counter : INT := 0;\n\
END_VAR\n\
    counter := counter + 1;\n\
END_PROGRAM\n";

const BROKEN_PROGRAM: &str = "\
PROGRAM Main\n\
VAR\n\
    x : INT := 0;\n\
END_VAR\n\
    x := undeclared;\n\
END_PROGRAM\n";

// ── --help / --version / unknown ─────────────────────────────────────────

#[test]
fn help_lists_all_subcommands() {
    let out = run(&["--help"]);
    assert!(out.status.success(), "--help should exit 0");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    for sub in ["agent", "debug", "run", "check", "version"] {
        assert!(
            combined.contains(sub),
            "help should advertise `{sub}` subcommand: {combined}"
        );
    }
}

#[test]
fn no_args_exits_with_usage() {
    let out = run(&[]);
    assert!(!out.status.success(), "no args should exit non-zero (clap)");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("usage") || stderr.to_lowercase().contains("subcommand"),
        "expected usage hint, got: {stderr}"
    );
}

#[test]
fn unknown_subcommand_exits_with_clap_error() {
    let out = run(&["nope-not-real"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("error") || stderr.contains("unrecognized"),
        "should produce a clap error, got: {stderr}"
    );
}

// ── version subcommand ───────────────────────────────────────────────────

#[test]
fn version_subcommand_prints_target_triple() {
    let out = run(&["version"]);
    assert!(out.status.success(), "`version` should exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("st-runtime"), "expected version banner, got: {stdout}");
    assert!(stdout.contains("Target:"), "expected `Target:` line, got: {stdout}");
    // Must include the actual host arch so we know the build is real.
    assert!(
        stdout.contains(std::env::consts::ARCH),
        "version should include arch {} : {stdout}",
        std::env::consts::ARCH,
    );
}

// ── check subcommand ─────────────────────────────────────────────────────

#[test]
fn check_clean_project_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: SmokeProj\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), VALID_PROGRAM);

    let out = run(&["check", dir.path().to_str().unwrap()]);
    assert!(out.status.success(), "check should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("OK"), "expected OK summary, got: {err}");
}

#[test]
fn check_broken_project_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: BrokenProj\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), BROKEN_PROGRAM);

    let out = run(&["check", dir.path().to_str().unwrap()]);
    assert!(!out.status.success(), "broken project should exit non-zero");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.to_lowercase().contains("error"),
        "should print at least one error, got: {err}"
    );
}

#[test]
fn check_nonexistent_path_errors() {
    let out = run(&["check", "/this/path/does/not/exist/probably"]);
    assert!(!out.status.success());
}

// ── run subcommand ───────────────────────────────────────────────────────

#[test]
fn run_executes_one_cycle() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: RunSmoke\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), VALID_PROGRAM);

    let out = run(&["run", dir.path().to_str().unwrap(), "--cycles", "1"]);
    assert!(out.status.success(), "run --cycles 1 should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Executed") && err.contains("cycle"),
        "expected cycle summary, got: {err}"
    );
}

#[test]
fn run_on_broken_project_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: BrokenRun\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), BROKEN_PROGRAM);

    let out = run(&["run", dir.path().to_str().unwrap(), "--cycles", "1"]);
    assert!(!out.status.success(), "broken project should not run");
}
