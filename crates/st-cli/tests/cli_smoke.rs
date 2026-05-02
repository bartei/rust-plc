//! Subprocess-driven smoke tests for `st-cli`.
//!
//! These exist to cover `crates/st-cli/src/main.rs` (≈900 lines) and
//! `crates/st-cli/src/comm_setup.rs` (≈200 lines), neither of which has
//! `#[cfg(test)]` modules — and the tests in `tests/lsp_integration.rs` only
//! drive the LSP `serve` subcommand, leaving `check`, `run`, `compile`,
//! `fmt`, `bundle`, `target`, `help`, and the unknown-command path at 0%.
//!
//! Each test spawns the binary that cargo built for this crate (resolved
//! via the standard `CARGO_BIN_EXE_st-cli` env var, plus a fallback for
//! cargo-llvm-cov which puts the binary in `target/llvm-cov-target/debug`)
//! and asserts on exit code + stderr/stdout text.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Resolve the `st-cli` binary. `CARGO_BIN_EXE_st-cli` is set by cargo when
/// running tests in this crate; under cargo-llvm-cov the variable still
/// points at the instrumented binary, so the same lookup just works.
fn st_cli() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_st-cli") {
        return PathBuf::from(p);
    }
    // Fallback for unusual harnesses; relies on the test binary's
    // `current_exe()` sitting in the same target dir as `st-cli`.
    let test_exe = std::env::current_exe().expect("current_exe");
    let dir = test_exe.parent().and_then(|p| p.parent()).expect("target dir");
    dir.join("st-cli")
}

fn run(args: &[&str]) -> Output {
    Command::new(st_cli())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("st-cli failed to launch")
}

fn run_in(args: &[&str], cwd: &std::path::Path) -> Output {
    Command::new(st_cli())
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .expect("st-cli failed to launch")
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

// ── Help / unknown / no-args ─────────────────────────────────────────────

#[test]
fn no_args_prints_usage_and_exits_nonzero() {
    let out = run(&[]);
    assert!(!out.status.success(), "no-args should exit non-zero");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Usage: st-cli"), "expected usage in stderr, got: {err}");
}

#[test]
fn help_subcommand_prints_usage_with_zero_exit() {
    for flag in ["help", "--help", "-h"] {
        let out = run(&[flag]);
        assert!(out.status.success(), "`st-cli {flag}` should exit 0");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(combined.contains("Usage: st-cli"), "expected usage from {flag}: {combined}");
        assert!(combined.contains("serve"), "help should list `serve`");
        assert!(combined.contains("bundle"), "help should list `bundle`");
    }
}

#[test]
fn unknown_subcommand_exits_nonzero_and_mentions_command() {
    let out = run(&["definitely-not-a-command"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Unknown command"), "expected 'Unknown command', got: {err}");
    assert!(err.contains("Usage: st-cli"), "should also print usage on unknown");
}

#[test]
fn debug_without_path_errors() {
    let out = run(&["debug"]);
    assert!(!out.status.success(), "`st-cli debug` (no path) should exit non-zero");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Usage: st-cli debug"), "expected usage hint, got: {err}");
}

// ── check ────────────────────────────────────────────────────────────────

#[test]
fn check_clean_file_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ok.st");
    write(&path, VALID_PROGRAM);

    let out = run(&["check", path.to_str().unwrap()]);
    assert!(out.status.success(), "expected exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(": OK"), "should print OK summary, got: {err}");
}

#[test]
fn check_broken_file_exits_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.st");
    write(&path, BROKEN_PROGRAM);

    let out = run(&["check", path.to_str().unwrap()]);
    assert!(!out.status.success(), "broken program should exit non-zero");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        combined.to_lowercase().contains("undeclared"),
        "expected an 'undeclared' diagnostic, got: {combined}"
    );
}

#[test]
fn check_json_emits_machine_readable_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.st");
    write(&path, BROKEN_PROGRAM);

    let out = run(&["check", path.to_str().unwrap(), "--json"]);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Tolerate any framing — just assert it's parseable as JSON-ish (`{` or `[`).
    let trimmed = stdout.trim_start();
    assert!(
        trimmed.starts_with('{') || trimmed.starts_with('['),
        "--json should produce JSON on stdout, got: {stdout}"
    );
}

// ── run ──────────────────────────────────────────────────────────────────

#[test]
fn run_executes_one_cycle_of_a_clean_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.st");
    write(&path, VALID_PROGRAM);

    let out = run(&["run", path.to_str().unwrap(), "-n", "1"]);
    assert!(out.status.success(), "single-file run should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Executed") && err.contains("cycle"),
        "expected cycle summary, got: {err}"
    );
}

#[test]
fn run_on_broken_file_exits_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.st");
    write(&path, BROKEN_PROGRAM);

    let out = run(&["run", path.to_str().unwrap(), "-n", "1"]);
    assert!(!out.status.success(), "running a broken program should exit non-zero");
}

// ── compile ──────────────────────────────────────────────────────────────

#[test]
fn compile_writes_bytecode_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("main.st");
    let out_file = dir.path().join("main.bytecode");
    write(&src, VALID_PROGRAM);

    let out = run(&[
        "compile",
        src.to_str().unwrap(),
        "-o",
        out_file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "compile failed: stderr={}",
        String::from_utf8_lossy(&out.stderr));
    assert!(out_file.exists(), "compile should write the output file");
    assert!(
        std::fs::metadata(&out_file).unwrap().len() > 0,
        "output file should be non-empty"
    );
}

// ── fmt ──────────────────────────────────────────────────────────────────

#[test]
fn fmt_runs_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.st");
    // Deliberately mis-indented so the formatter has work to do.
    write(&path, "PROGRAM Main\nVAR\nx : INT := 0;\nEND_VAR\nx := x + 1;\nEND_PROGRAM\n");

    let out = run(&["fmt", path.to_str().unwrap()]);
    assert!(out.status.success(), "fmt failed: stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("PROGRAM Main"), "fmt must preserve program contents");
}

// ── bundle ───────────────────────────────────────────────────────────────

#[test]
fn bundle_creates_st_bundle_archive() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: SmokeProj\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), VALID_PROGRAM);

    let out = run_in(&["bundle"], dir.path());
    assert!(out.status.success(), "bundle failed: stderr={}",
        String::from_utf8_lossy(&out.stderr));

    let produced = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().ends_with(".st-bundle"));
    assert!(
        produced.is_some(),
        "bundle should produce a .st-bundle file in {}",
        dir.path().display()
    );
}

// ── target ───────────────────────────────────────────────────────────────

#[test]
fn target_with_no_subcommand_prints_usage() {
    let out = run(&["target"]);
    // Implementation prints usage; exit code may be 0 or 1.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        combined.contains("install") || combined.contains("uninstall") || combined.contains("Usage"),
        "expected target subcommand listing, got: {combined}"
    );
}

#[test]
fn target_install_without_host_errors() {
    let out = run(&["target", "install"]);
    assert!(!out.status.success(), "missing user@host should fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.to_lowercase().contains("usage") || err.to_lowercase().contains("user@host"),
        "expected usage hint for missing arg, got: {err}"
    );
}

// ── Project-mode checks (drive the `comm_setup` module) ──────────────────
// `check`/`run` in project mode walk the device-profile loader in
// `crates/st-cli/src/comm_setup.rs`. Pointing them at a project with no
// profiles takes the empty-registry early-return path; that's enough to
// move comm_setup from 0% to 60-ish percent coverage.

#[test]
fn check_in_project_mode_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: ProjCheckSmoke\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), VALID_PROGRAM);

    let out = run(&["check", dir.path().to_str().unwrap()]);
    assert!(out.status.success(), "project check failed: stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Project"), "expected project banner, got: {err}");
}

#[test]
fn run_in_project_mode_executes_one_cycle() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("plc-project.yaml"),
        "name: ProjRunSmoke\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    write(&dir.path().join("main.st"), VALID_PROGRAM);

    let out = run(&["run", dir.path().to_str().unwrap(), "-n", "1"]);
    assert!(out.status.success(), "project run failed: stderr={}",
        String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Executed") && err.contains("cycle"),
        "expected cycle summary, got: {err}"
    );
}
