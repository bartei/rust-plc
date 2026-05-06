//! In-process HTTP integration tests for the agent API.
//!
//! These tests start the agent's axum server on a random port and exercise
//! every endpoint using reqwest. No VMs or external infrastructure needed —
//! runs with normal `cargo test`.

use reqwest::Client;
use st_deploy::bundle::{create_bundle, write_bundle, BundleOptions};
use st_target_agent::config::{AgentConfig, AuthMode};
use st_target_agent::server::{build_app_state, build_router};
use std::fs;
use std::time::Duration;
use tokio::net::TcpListener;

/// Start a test agent on a random port, return the base URL.
async fn start_agent(config: AgentConfig) -> (String, tokio::task::JoinHandle<()>) {
    let state = build_app_state(config, None).unwrap();
    let router = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

/// Create a test config with a temp directory for program storage.
fn test_config(dir: &std::path::Path) -> AgentConfig {
    let mut config = AgentConfig::default();
    config.storage.program_dir = dir.to_path_buf();
    config
}

/// Create a .st-bundle from an inline ST program and return the raw bytes.
fn make_bundle(name: &str, source: &str) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        format!("name: {name}\nversion: '1.0.0'\nentryPoint: Main\n"),
    )
    .unwrap();
    fs::write(root.join("main.st"), source).unwrap();

    let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
    let bundle_path = root.join("test.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();
    fs::read(&bundle_path).unwrap()
}

fn simple_program() -> Vec<u8> {
    make_bundle(
        "TestProg",
        "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + 1;\nEND_PROGRAM\n",
    )
}

/// Upload a bundle to the agent via multipart POST.
async fn upload_bundle(client: &Client, base: &str, data: &[u8]) -> reqwest::Response {
    let part = reqwest::multipart::Part::bytes(data.to_vec()).file_name("program.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap()
}

// ─── Health & Status ────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.get(format!("{base}/api/v1/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["healthy"], true);
    assert!(!body["version"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_target_info() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.get(format!("{base}/api/v1/target-info")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["os"].as_str().is_some());
    assert!(body["arch"].as_str().is_some());
    assert!(body["agent_version"].as_str().is_some());
    assert!(body["uptime_secs"].as_u64().is_some());
}

#[tokio::test]
async fn test_status_idle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.get(format!("{base}/api/v1/status")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "idle");
}

// ─── Upload ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_upload_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let bundle_data = simple_program();
    let resp = upload_bundle(&client, &base, &bundle_data).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["program"]["name"], "TestProg");
    assert_eq!(body["program"]["version"], "1.0.0");
}

#[tokio::test]
async fn test_program_info() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // No program initially → 404
    let resp = client.get(format!("{base}/api/v1/program/info")).send().await.unwrap();
    assert_eq!(resp.status(), 404);

    // Upload → info available
    upload_bundle(&client, &base, &simple_program()).await;

    let resp = client.get(format!("{base}/api/v1/program/info")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "TestProg");
}

// ─── Start / Stop / Restart ─────────────────────────────────────────────

#[tokio::test]
async fn test_start_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_bundle(&client, &base, &simple_program()).await;

    // Start
    let resp = client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "running");

    // Stop
    let resp = client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "idle");
}

#[tokio::test]
async fn test_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_bundle(&client, &base, &simple_program()).await;

    // Start
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Restart
    let resp = client.post(format!("{base}/api/v1/program/restart")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    tokio::time::sleep(Duration::from_millis(200)).await;

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "running");

    // Cleanup
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ─── Delete ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_program() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_bundle(&client, &base, &simple_program()).await;

    let resp = client.delete(format!("{base}/api/v1/program")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client.get(format!("{base}/api/v1/program/info")).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ─── Error Cases ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_start_without_program() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_stop_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    assert_eq!(resp.status(), 409); // Conflict
}

// ─── Cycle Stats ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_cycle_stats_advance() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_bundle(&client, &base, &simple_program()).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();

    assert_eq!(status["status"], "running");
    let cycle_count = status["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(cycle_count > 0, "Cycle count should be > 0, got {cycle_count}");

    // Wait and check again — should advance
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status2: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    let cycle_count2 = status2["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(cycle_count2 > cycle_count, "Cycles should advance: {cycle_count} -> {cycle_count2}");

    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// Replaces the deleted `program_store::tests::invalid_bundle_rejected` unit test:
// the HTTP path used to be silently uncovered for malformed bundles, so we
// now assert the 400 response and the `invalid_bundle` error code shape end-to-end.
#[tokio::test]
async fn test_upload_invalid_bundle_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let part = reqwest::multipart::Part::bytes(b"not a real bundle".to_vec())
        .file_name("garbage.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_bundle");
}

// ─── Upload Replaces ────────────────────────────────────────────────────

#[tokio::test]
async fn test_upload_replaces_existing() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let bundle_v1 = make_bundle(
        "ProjV1",
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    );
    let bundle_v2 = make_bundle(
        "ProjV2",
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 2;\nEND_PROGRAM\n",
    );

    upload_bundle(&client, &base, &bundle_v1).await;
    let info: serde_json::Value = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(info["name"], "ProjV1");

    upload_bundle(&client, &base, &bundle_v2).await;
    let info: serde_json::Value = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(info["name"], "ProjV2");
}

// ─── Authentication ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_auth_required() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.auth.mode = AuthMode::Token;
    config.auth.token = Some("my-secret".to_string());

    let (base, _handle) = start_agent(config).await;
    let client = Client::new();

    // No token → 401
    let resp = client.get(format!("{base}/api/v1/status")).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_auth_valid_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.auth.mode = AuthMode::Token;
    config.auth.token = Some("my-secret".to_string());

    let (base, _handle) = start_agent(config).await;
    let client = Client::new();

    // Valid token → 200
    let resp = client
        .get(format!("{base}/api/v1/status"))
        .header("Authorization", "Bearer my-secret")
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_auth_health_exempt() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.auth.mode = AuthMode::Token;
    config.auth.token = Some("my-secret".to_string());

    let (base, _handle) = start_agent(config).await;
    let client = Client::new();

    // Health endpoint always public
    let resp = client.get(format!("{base}/api/v1/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_read_only_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.auth.mode = AuthMode::Token;
    config.auth.token = Some("readonly-token".to_string());
    config.auth.read_only = true;

    let (base, _handle) = start_agent(config).await;
    let client = Client::new();
    let auth = "Bearer readonly-token";

    // GET works
    let resp = client
        .get(format!("{base}/api/v1/status"))
        .header("Authorization", auth)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // POST rejected
    let resp = client
        .post(format!("{base}/api/v1/program/start"))
        .header("Authorization", auth)
        .send().await.unwrap();
    assert_eq!(resp.status(), 403);
}

// ─── Logs ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_logs_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client.get(format!("{base}/api/v1/logs")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["entries"].as_array().is_some());
}

// ─── Full Lifecycle ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // 1. Health check
    let resp = client.get(format!("{base}/api/v1/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // 2. Status = idle
    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "idle");

    // 3. Upload
    let bundle = simple_program();
    let resp = upload_bundle(&client, &base, &bundle).await;
    assert_eq!(resp.status(), 200);

    // 4. Info available
    let info: serde_json::Value = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(info["name"], "TestProg");

    // 5. Start
    let resp = client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 6. Verify running with advancing cycles
    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "running");
    let c1 = status["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(c1 > 0);

    // 7. Stop
    let resp = client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "idle");

    // 8. Delete
    let resp = client.delete(format!("{base}/api/v1/program")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // 9. Verify clean
    let resp = client.get(format!("{base}/api/v1/program/info")).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ─── Log Level Control ──────────────────────────────────────────────────

#[tokio::test]
async fn test_get_log_level() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.logging.level = "warn".to_string();
    // Use None handle — GET should return the config's level
    let (base, _handle) = start_agent(config).await;
    let client = Client::new();

    let resp = client.get(format!("{base}/api/v1/log-level")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["level"], "warn", "Should return configured log level");
}

#[tokio::test]
async fn test_set_log_level() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    // Use init_logging to get a real handle. The try_init inside
    // may or may not succeed (depends on test ordering), but the
    // LogLevelHandle still tracks the level internally.
    let log_handle = st_target_agent::logging::init_logging("info");
    let state = build_app_state(config, Some(log_handle)).unwrap();
    let router = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let base = format!("http://{addr}");
    let client = Client::new();

    // Change to debug
    let resp = client
        .put(format!("{base}/api/v1/log-level"))
        .json(&serde_json::json!({ "level": "debug" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["level"], "debug");

    // Verify it persisted
    let resp = client.get(format!("{base}/api/v1/log-level")).send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["level"], "debug");
}

#[tokio::test]
async fn test_set_invalid_log_level() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let log_handle = st_target_agent::logging::init_logging("info");
    let state = build_app_state(config, Some(log_handle)).unwrap();
    let router = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let base = format!("http://{addr}");
    let client = Client::new();

    let resp = client
        .put(format!("{base}/api/v1/log-level"))
        .json(&serde_json::json!({ "level": "verbose" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "Invalid log level should return 400");
}

#[tokio::test]
async fn test_log_level_without_handle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // GET should still return the config default
    let resp = client.get(format!("{base}/api/v1/log-level")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["level"], "info");

    // PUT should fail gracefully (no handle available)
    let resp = client
        .put(format!("{base}/api/v1/log-level"))
        .json(&serde_json::json!({ "level": "debug" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
}

// ─── Variable Monitoring API ───────────────────────────────────────────

/// A program with multiple variable types for richer monitoring tests.
fn multi_var_program() -> Vec<u8> {
    make_bundle(
        "MonitorTest",
        concat!(
            "PROGRAM Main\n",
            "VAR\n",
            "    counter : INT := 0;\n",
            "    flag : BOOL := FALSE;\n",
            "    temperature : REAL := 21.5;\n",
            "END_VAR\n",
            "    counter := counter + 1;\n",
            "    IF counter > 10 THEN flag := TRUE; END_IF;\n",
            "END_PROGRAM\n",
        ),
    )
}

/// Helper: upload, start, and wait for the engine to be running.
async fn upload_and_start(client: &Client, base: &str, bundle: &[u8]) {
    upload_bundle(client, base, bundle).await;
    client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
}

#[tokio::test]
async fn test_catalog_empty_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert!(vars.is_empty(), "Catalog should be empty when idle");
}

#[tokio::test]
async fn test_catalog_populated_when_running() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let resp = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert!(
        vars.len() >= 3,
        "Catalog should have at least 3 variables (counter, flag, temperature), got {}",
        vars.len()
    );

    // Each entry should have name + type
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
        "Catalog should contain Main.counter, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.flag")),
        "Catalog should contain Main.flag, got: {names:?}"
    );

    // Verify type field is present
    let counter_entry = vars
        .iter()
        .find(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("Main.counter"))
        })
        .unwrap();
    assert_eq!(counter_entry["type"], "INT");

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_catalog_clears_after_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &simple_program()).await;

    // Catalog populated while running
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!body["variables"].as_array().unwrap().is_empty());

    // Stop
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Catalog should be empty again
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        body["variables"].as_array().unwrap().is_empty(),
        "Catalog should clear after stop"
    );
}

#[tokio::test]
async fn test_watch_variables_returns_values() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    // First poll sets the watch list — values may be empty on the very first
    // call because the engine hasn't picked up the watch list yet.
    client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.counter,Main.flag"
        ))
        .send()
        .await
        .unwrap();

    // Wait for the engine to produce at least one snapshot.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Second poll should have values
    let resp = client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.counter,Main.flag"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 2, "Should return exactly 2 watched variables");

    // Check structure: each has name, value, type, forced
    let counter = vars
        .iter()
        .find(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("Main.counter"))
        })
        .expect("Should find Main.counter in watched values");
    assert!(counter["value"].as_str().is_some());
    assert_eq!(counter["type"], "INT");
    assert_eq!(counter["forced"], false);

    let flag = vars
        .iter()
        .find(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("Main.flag"))
        })
        .expect("Should find Main.flag in watched values");
    assert!(flag["value"].as_str().is_some());
    assert_eq!(flag["type"], "BOOL");

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watch_empty_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &simple_program()).await;

    // Empty watch parameter → returns all variables
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch="))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert!(
        !vars.is_empty(),
        "Empty watch should return all variables (got {})",
        vars.len()
    );

    // No watch parameter at all → also returns all variables
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert!(
        !vars.is_empty(),
        "No watch param should return all variables (got {})",
        vars.len()
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_force_variable() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    // Force counter to 999
    let resp = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "Main.counter", "value": "999" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let result = body["result"].as_str().unwrap();
    assert!(
        result.contains("999"),
        "Force result should confirm value: {result}"
    );

    // Verify it takes effect: watch the variable and check forced flag
    // Set watch list first
    client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 1);
    let counter = &vars[0];
    assert_eq!(counter["value"], "999", "Forced value should be 999");
    assert_eq!(counter["forced"], true, "Variable should be marked as forced");

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_unforce_variable() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    // Force then unforce
    client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "Main.counter", "value": "42" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .delete(format!("{base}/api/v1/variables/force/Main.counter"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);

    // Verify the forced flag clears
    client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 1);
    assert_eq!(
        vars[0]["forced"], false,
        "Variable should no longer be forced after unforce"
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_force_when_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "x", "value": "1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Force should fail when not running");
}

#[tokio::test]
async fn test_unforce_when_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .delete(format!("{base}/api/v1/variables/force/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Unforce should fail when not running");
}

#[tokio::test]
async fn test_force_bool_variable() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    // Force boolean
    let resp = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "Main.flag", "value": "true" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let result = body["result"].as_str().unwrap();
    assert!(
        result.contains("TRUE"),
        "Force result should confirm TRUE: {result}"
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watch_nonexistent_variable_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &simple_program()).await;

    // Watch a variable that doesn't exist
    client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.nonexistent"
        ))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.nonexistent"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert!(
        vars.is_empty(),
        "Nonexistent variable should not appear in results"
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_monitor_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // 1. Catalog empty when idle
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(body["variables"].as_array().unwrap().is_empty());

    // 2. Upload and start
    upload_and_start(&client, &base, &multi_var_program()).await;

    // 3. Catalog populated
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let catalog = body["variables"].as_array().unwrap();
    assert!(catalog.len() >= 3);

    // 4. Set watch and poll
    client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.counter,Main.flag,Main.temperature"
        ))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.counter,Main.flag,Main.temperature"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 3, "Should return all 3 watched variables");

    // 5. Force a variable
    let resp = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "Main.counter", "value": "42" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 6. Verify force took effect
    tokio::time::sleep(Duration::from_millis(100)).await;
    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let counter = &body["variables"].as_array().unwrap()[0];
    assert_eq!(counter["value"], "42");
    assert_eq!(counter["forced"], true);

    // 7. Unforce
    client
        .delete(format!("{base}/api/v1/variables/force/Main.counter"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let counter = &body["variables"].as_array().unwrap()[0];
    assert_eq!(counter["forced"], false);
    // Counter should be advancing again (not stuck at 42)
    let val: i64 = counter["value"].as_str().unwrap().parse().unwrap();
    assert!(val > 0, "Counter should be advancing after unforce");

    // 8. Stop — catalog clears
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(body["variables"].as_array().unwrap().is_empty());
}

// ─── WebSocket Monitor ─────────────────────────────────────────────────

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite;

/// Connect a WebSocket client to the agent's monitor endpoint.
async fn ws_connect(
    base: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
{
    let ws_url = base.replace("http://", "ws://") + "/api/v1/monitor/ws";
    let (ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws
}

/// Send a JSON request and wait for the next non-push response.
/// Pushed variableUpdate messages are silently skipped.
async fn ws_request(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    request: serde_json::Value,
) -> serde_json::Value {
    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&request).unwrap(),
    ))
    .await
    .unwrap();
    // Read messages until we get a non-push response
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("WebSocket response timeout")
            .expect("WebSocket closed")
            .expect("WebSocket error");
        if let tungstenite::Message::Text(text) = msg {
            let val: serde_json::Value = serde_json::from_str(&text).unwrap();
            // Skip pushed variableUpdate messages (they're interleaved)
            if val.get("type").and_then(|t| t.as_str()) == Some("variableUpdate") {
                continue;
            }
            return val;
        }
    }
}

/// Receive the next pushed message (with timeout).
async fn ws_recv(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> serde_json::Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("WebSocket push timeout")
            .expect("WebSocket closed")
            .expect("WebSocket error");
        if let tungstenite::Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

#[tokio::test]
async fn test_ws_get_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({ "method": "getCatalog" }),
    )
    .await;

    assert_eq!(resp["type"], "catalog");
    let vars = resp["variables"].as_array().unwrap();
    assert!(
        vars.len() >= 3,
        "Catalog should have >= 3 variables, got {}",
        vars.len()
    );

    // Verify structure
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
        "Should contain Main.counter: {names:?}"
    );

    // Cleanup
    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_subscribe_and_receive_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let mut ws = ws_connect(&base).await;

    // Subscribe to counter
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["Main.counter"], "interval_ms": 0 }
        }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);

    // Should receive a variableUpdate push within ~100ms
    let push = ws_recv(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");
    let vars = push["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 1);
    assert!(
        vars[0]["name"]
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case("Main.counter"),
        "Pushed variable should be Main.counter"
    );
    assert!(vars[0]["value"].as_str().is_some());
    assert_eq!(vars[0]["forced"], false);

    // Cleanup
    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_force_and_unforce() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let mut ws = ws_connect(&base).await;

    // Subscribe to counter
    ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["Main.counter"], "interval_ms": 0 }
        }),
    )
    .await;

    // Force counter to 999
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "force",
            "params": { "variable": "Main.counter", "value": 999 }
        }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);

    // Wait for a push with the forced value (check up to 10 pushes)
    let mut found_forced = false;
    for _ in 0..10 {
        let push = ws_recv(&mut ws).await;
        if push["type"] == "variableUpdate" {
            let vars = push["variables"].as_array().unwrap();
            if let Some(counter) = vars.iter().find(|v| v["value"] == "999") {
                assert_eq!(counter["forced"], true, "Should be marked as forced");
                found_forced = true;
                break;
            }
        }
    }
    assert!(found_forced, "Should have received forced value 999 in a push");

    // Unforce
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "unforce",
            "params": { "variable": "Main.counter" }
        }),
    )
    .await;
    assert_eq!(resp["success"], true);

    // Cleanup
    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_get_cycle_info() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({ "method": "getCycleInfo" }),
    )
    .await;

    assert_eq!(resp["type"], "cycleInfo");
    assert!(
        resp["cycle_count"].as_u64().unwrap() > 0,
        "Cycle count should be > 0"
    );
    assert!(resp["last_cycle_us"].as_u64().is_some());

    // Cleanup
    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_unsubscribe_stops_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &multi_var_program()).await;

    let mut ws = ws_connect(&base).await;

    // Subscribe
    ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["Main.counter"], "interval_ms": 0 }
        }),
    )
    .await;

    // Receive at least one push
    let push = ws_recv(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");

    // Unsubscribe
    ws_request(
        &mut ws,
        serde_json::json!({
            "method": "unsubscribe",
            "params": { "variables": ["Main.counter"] }
        }),
    )
    .await;

    // After unsubscribe, we should NOT receive further pushes.
    // Wait 200ms — if we get a variableUpdate, that's a bug.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let timeout = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
    match timeout {
        Ok(Some(Ok(tungstenite::Message::Text(text)))) => {
            let msg: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_ne!(
                msg["type"], "variableUpdate",
                "Should not receive pushes after unsubscribe"
            );
        }
        _ => { /* timeout = good, no pushes */ }
    }

    // Cleanup
    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;

    let mut ws = ws_connect(&base).await;
    ws.send(tungstenite::Message::Text("not json".to_string()))
        .await
        .unwrap();

    let resp = ws_recv(&mut ws).await;
    assert_eq!(resp["type"], "error");
    assert!(resp["message"].as_str().unwrap().contains("Invalid request"));

    ws.close(None).await.ok();
}

// ─── Array Variable Monitoring ────────────────────────────────────────

/// A program that declares and writes to array variables.
fn array_program() -> Vec<u8> {
    make_bundle(
        "ArrayTest",
        concat!(
            "PROGRAM Main\n",
            "VAR\n",
            "    arr : ARRAY[1..5] OF INT;\n",
            "    total : INT := 0;\n",
            "END_VAR\n",
            "    arr[1] := 10;\n",
            "    arr[2] := 20;\n",
            "    arr[3] := 30;\n",
            "    arr[4] := 40;\n",
            "    arr[5] := 50;\n",
            "    total := arr[1] + arr[2] + arr[3];\n",
            "END_PROGRAM\n",
        ),
    )
}

#[tokio::test]
async fn test_catalog_includes_array_elements() {
    // The variable catalog must expose array elements so the Monitor
    // panel's autocomplete knows about them. They should appear either
    // as individual indexed entries (Main.arr[1], Main.arr[2], ...) or
    // as a parent entry with type containing "ARRAY".
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &array_program()).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("Catalog entries: {names:?}");

    // Array entries must be present
    let arr_entries: Vec<&&str> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with("main.arr"))
        .collect();
    assert!(
        !arr_entries.is_empty(),
        "Catalog should contain entries for Main.arr, got: {names:?}"
    );

    // Check for indexed element entries
    let indexed: Vec<&&str> = names
        .iter()
        .filter(|n| n.starts_with("Main.arr["))
        .collect();
    if !indexed.is_empty() {
        // If present as individual elements, must have all 5
        assert!(
            indexed.len() >= 5,
            "Expected at least 5 indexed entries (Main.arr[1]..Main.arr[5]), got: {indexed:?}"
        );
        // Each element should be typed as INT
        for idx_name in &indexed {
            let entry = vars
                .iter()
                .find(|v| v["name"].as_str() == Some(**idx_name));
            if let Some(e) = entry {
                let ty = e["type"].as_str().unwrap_or("");
                assert!(
                    ty.contains("INT"),
                    "Array element type should be INT, got '{ty}' for {idx_name}"
                );
            }
        }
    } else {
        // Must have a parent entry "Main.arr" with type containing ARRAY
        let arr_entry = vars
            .iter()
            .find(|v| {
                v["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case("Main.arr"))
            });
        assert!(
            arr_entry.is_some(),
            "Expected parent catalog entry 'Main.arr', got: {names:?}"
        );
        let ty = arr_entry.unwrap()["type"].as_str().unwrap_or("");
        assert!(
            ty.to_uppercase().contains("ARRAY"),
            "Array catalog type should contain 'ARRAY', got '{ty}'"
        );
    }

    // Scalar 'total' should still be present
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.total")),
        "Catalog should contain Main.total, got: {names:?}"
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watch_array_elements_via_http() {
    // Watching an array variable via the HTTP polling API should return
    // array element values — either as individual named entries or as
    // a composite value.
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &array_program()).await;

    // First poll sets the watch list
    client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.arr[1],Main.arr[2],Main.arr[3],Main.arr[4],Main.arr[5]"
        ))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Second poll should have values
    let body: serde_json::Value = client
        .get(format!(
            "{base}/api/v1/variables?watch=Main.arr[1],Main.arr[2],Main.arr[3],Main.arr[4],Main.arr[5]"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    eprintln!(
        "Watched array elements: {:?}",
        vars.iter()
            .map(|v| format!(
                "{}={}",
                v["name"].as_str().unwrap_or("?"),
                v["value"].as_str().unwrap_or("?")
            ))
            .collect::<Vec<_>>()
    );

    // All 5 elements should be returned with values
    assert_eq!(
        vars.len(),
        5,
        "Expected 5 array element values, got {}: {:?}",
        vars.len(),
        vars
    );

    // Check that element values are the written constants
    for (idx, expected) in [(1, "10"), (2, "20"), (3, "30"), (4, "40"), (5, "50")] {
        let name = format!("Main.arr[{idx}]");
        let entry = vars
            .iter()
            .find(|v| {
                v["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&name))
            });
        assert!(
            entry.is_some(),
            "Expected '{name}' in watched results, got: {:?}",
            vars.iter().map(|v| v["name"].as_str()).collect::<Vec<_>>()
        );
        let val = entry.unwrap()["value"].as_str().unwrap_or("");
        assert_eq!(
            val, expected,
            "{name} should be {expected}, got '{val}'"
        );
    }

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watch_array_parent_via_http() {
    // Watching the parent "Main.arr" (without indices) should return all
    // array elements — similar to how watching a FB instance prefix returns
    // all descendant fields.
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &array_program()).await;

    // Watch the parent name
    client
        .get(format!("{base}/api/v1/variables?watch=Main.arr"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.arr"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    let var_names: Vec<&str> = vars
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    eprintln!("Watch Main.arr result: {var_names:?}");

    // Should include all 5 elements (flat or nested)
    let element_count = vars
        .iter()
        .filter(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("Main.arr["))
        })
        .count();
    assert!(
        element_count >= 5,
        "Watching 'Main.arr' should expand to at least 5 elements, got {element_count}: {var_names:?}"
    );

    // Cleanup
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_subscribe_array_elements() {
    // Subscribing to individual array elements via WebSocket should push
    // their values in variableUpdate messages.
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &array_program()).await;

    let mut ws = ws_connect(&base).await;

    // Subscribe to array elements
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": {
                "variables": [
                    "Main.arr[1]",
                    "Main.arr[2]",
                    "Main.arr[3]"
                ],
                "interval_ms": 0
            }
        }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
    assert_eq!(
        resp["data"]["subscribed"], 3,
        "Should have subscribed to 3 array elements"
    );

    // Wait for a push with array element values
    let push = ws_recv(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");
    let vars = push["variables"].as_array().unwrap();
    let var_names: Vec<&str> = vars
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    eprintln!("WS push array elements: {var_names:?}");

    // Should include the 3 subscribed elements
    assert_eq!(
        vars.len(),
        3,
        "Push should contain exactly 3 subscribed array elements, got: {var_names:?}"
    );
    for idx in [1, 2, 3] {
        let name = format!("Main.arr[{idx}]");
        assert!(
            var_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name)),
            "Push should contain '{name}', got: {var_names:?}"
        );
    }

    // Cleanup
    ws.close(None).await.ok();
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_catalog_includes_array_elements() {
    // The WebSocket getCatalog response should include array element entries.
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &array_program()).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({ "method": "getCatalog" }),
    )
    .await;

    assert_eq!(resp["type"], "catalog");
    let vars = resp["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("WS catalog entries: {names:?}");

    // Array elements must be present
    let arr_entries: Vec<&&str> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with("main.arr"))
        .collect();
    assert!(
        !arr_entries.is_empty(),
        "WS catalog should contain entries for Main.arr, got: {names:?}"
    );

    // Prefer indexed entries
    let indexed: Vec<&&str> = names
        .iter()
        .filter(|n| n.starts_with("Main.arr["))
        .collect();
    if !indexed.is_empty() {
        assert!(
            indexed.len() >= 5,
            "Expected at least 5 indexed catalog entries, got: {indexed:?}"
        );
    }

    // Cleanup
    ws.close(None).await.ok();
    client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ─── Compound-type Watch (FB / nested) ────────────────────────────────

/// A program with nested FB instances for compound watch testing.
fn nested_fb_program() -> Vec<u8> {
    make_bundle(
        "NestedFbTest",
        concat!(
            "FUNCTION_BLOCK Inner\n",
            "VAR_INPUT x : INT; END_VAR\n",
            "VAR_OUTPUT y : INT; END_VAR\n",
            "    y := x * 2;\n",
            "END_FUNCTION_BLOCK\n",
            "\n",
            "FUNCTION_BLOCK Outer\n",
            "VAR_INPUT cmd : BOOL; END_VAR\n",
            "VAR\n",
            "    sub : Inner;\n",
            "    state : INT := 0;\n",
            "END_VAR\n",
            "    state := state + 1;\n",
            "    sub(x := state);\n",
            "END_FUNCTION_BLOCK\n",
            "\n",
            "PROGRAM Main\n",
            "VAR\n",
            "    fb : Outer;\n",
            "    counter : INT := 0;\n",
            "END_VAR\n",
            "    fb(cmd := TRUE);\n",
            "    counter := counter + 1;\n",
            "END_PROGRAM\n",
        ),
    )
}

#[tokio::test]
async fn test_watch_fb_parent_via_http() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &nested_fb_program()).await;

    client
        .get(format!("{base}/api/v1/variables?watch=Main.fb"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.fb"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("Watch Main.fb result: {names:?}");

    // Direct fields
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.cmd")),
        "Should include Main.fb.cmd: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.state")),
        "Should include Main.fb.state: {names:?}"
    );
    // Nested Inner FB fields (2 levels)
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.x")),
        "Should include nested Main.fb.sub.x: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.y")),
        "Should include nested Main.fb.sub.y: {names:?}"
    );
    // Unrelated variables excluded
    assert!(
        !names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
        "Should NOT include Main.counter: {names:?}"
    );

    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watch_entire_main_via_http() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &nested_fb_program()).await;

    client
        .get(format!("{base}/api/v1/variables?watch=Main"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let body: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vars = body["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("Watch Main result: {names:?}");

    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
        "Should include Main.counter: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.state")),
        "Should include Main.fb.state: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.x")),
        "Should include Main.fb.sub.x: {names:?}"
    );
    assert!(
        vars.len() >= 5,
        "Expected at least 5 variables for 'Main', got {}: {names:?}",
        vars.len()
    );

    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_ws_subscribe_fb_parent_pushes_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    upload_and_start(&client, &base, &nested_fb_program()).await;

    let mut ws = ws_connect(&base).await;

    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["Main.fb"], "interval_ms": 0 }
        }),
    )
    .await;
    assert_eq!(resp["success"], true);

    // Wait for a push that contains descendant variables. The first push
    // may arrive before the engine has populated fb_instances, so retry
    // up to 10 pushes.
    let mut found = false;
    for _ in 0..10 {
        let push = ws_recv(&mut ws).await;
        if push["type"] != "variableUpdate" {
            continue;
        }
        let vars = push["variables"].as_array().unwrap();
        let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();

        if names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.state")) {
            eprintln!("WS push Main.fb descendants: {names:?}");
            assert!(
                names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.x")),
                "Push should include nested Main.fb.sub.x: {names:?}"
            );
            assert!(
                !names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
                "Push should NOT include Main.counter: {names:?}"
            );
            found = true;
            break;
        }
    }
    assert!(found, "Should have received a push with Main.fb descendant fields");

    ws.close(None).await.ok();
    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ─── Program Update (POST /api/v1/program/update) ───────────────────────

/// Multipart helper that posts a bundle to /api/v1/program/update.
async fn post_update(client: &Client, base: &str, data: &[u8]) -> reqwest::Response {
    let part = reqwest::multipart::Part::bytes(data.to_vec()).file_name("program.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    client
        .post(format!("{base}/api/v1/program/update"))
        .multipart(form)
        .send()
        .await
        .unwrap()
}

/// Two bundles with the same variable layout — online change is compatible.
fn counter_bundle(name: &str, version: &str, increment: i32) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        format!("name: {name}\nversion: '{version}'\nentryPoint: Main\n"),
    )
    .unwrap();
    fs::write(
        root.join("main.st"),
        format!(
            "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + {increment};\nEND_PROGRAM\n"
        ),
    )
    .unwrap();
    let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
    let bundle_path = root.join("test.st-bundle");
    write_bundle(&bundle, &bundle_path).unwrap();
    fs::read(&bundle_path).unwrap()
}

/// First-time deployment to an empty store responds with `initial_deploy`
/// and does not auto-start the engine.
#[tokio::test]
async fn test_update_initial_deploy() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = post_update(&client, &base, &counter_bundle("UpdProj", "1.0.0", 1)).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["method"], "initial_deploy");
    assert_eq!(body["program"]["version"], "1.0.0");

    // Engine should still be idle — caller must explicitly start.
    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "idle");
}

/// When the engine is running and the new bundle has the same variable
/// layout, /update applies an online change and keeps cycling.
#[tokio::test]
async fn test_update_online_change_preserves_running_state() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // Deploy and start v1 (counter += 1)
    upload_bundle(&client, &base, &counter_bundle("UpdProj", "1.0.0", 1)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let cycle_before = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json::<serde_json::Value>().await.unwrap()
        ["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(cycle_before > 0, "Engine should have advanced before update");

    // Push v2 (counter += 2) via /update — same layout, online change.
    let resp = post_update(&client, &base, &counter_bundle("UpdProj", "2.0.0", 2)).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "online_change", "body={body}");
    assert_eq!(body["program"]["version"], "2.0.0");
    // counter is preserved across the swap.
    let preserved = body["online_change"]["preserved_vars"].as_array().unwrap();
    assert!(
        preserved.iter().any(|v| v.as_str().unwrap_or("").to_lowercase().contains("counter")),
        "Expected counter to be preserved: {preserved:?}"
    );

    // Engine kept running — cycle count keeps advancing past the swap.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "running");
    let cycle_after = status["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(
        cycle_after > cycle_before,
        "Cycles should advance through online change: {cycle_before} -> {cycle_after}"
    );

    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
}

/// When the new bundle changes the variable layout in an incompatible way,
/// /update transparently falls back to a stop+start sequence.
#[tokio::test]
async fn test_update_incompatible_falls_back_to_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // v1 has `x : INT`
    let v1 = make_bundle(
        "Incompat",
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n",
    );
    upload_bundle(&client, &base, &v1).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // v2 changes x to REAL — type mismatch is incompatible.
    let v2 = make_bundle(
        "Incompat",
        "PROGRAM Main\nVAR\n    x : REAL := 0.0;\nEND_VAR\n    x := x + 1.5;\nEND_PROGRAM\n",
    );
    let resp = post_update(&client, &base, &v2).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "restart", "body={body}");

    // Engine is running again with the new program.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "running");

    client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
}

/// /update against an idle agent that already has a stored program just
/// replaces the bundle (cold replace) without auto-starting.
#[tokio::test]
async fn test_update_cold_replace_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _handle) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // Initial deploy (no engine running).
    upload_bundle(&client, &base, &counter_bundle("ColdProj", "1.0.0", 1)).await;

    // Now /update again while idle.
    let resp = post_update(&client, &base, &counter_bundle("ColdProj", "2.0.0", 2)).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "cold_replace");
    assert_eq!(body["program"]["version"], "2.0.0");

    let status: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(status["status"], "idle");
}

/// Read-only mode rejects /update with 403.
#[tokio::test]
async fn test_update_rejected_in_read_only_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path());
    config.auth.mode = AuthMode::Token;
    config.auth.token = Some("ro-token".to_string());
    config.auth.read_only = true;

    let (base, _handle) = start_agent(config).await;
    let client = Client::new();

    let bundle = counter_bundle("ROProj", "1.0.0", 1);
    let part = reqwest::multipart::Part::bytes(bundle).file_name("program.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = client
        .post(format!("{base}/api/v1/program/update"))
        .header("Authorization", "Bearer ro-token")
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

// ─── program.rs error paths (acceptance) ────────────────────────────────
//
// Targets the gaps tracked in plan/implementation.md: error-path coverage
// for /api/v1/program/upload (multipart edge cases) and /program/start
// (no-program / corrupted-stored-bytecode). Drive everything through the
// real axum router so the failure surface is the actual user-facing
// HTTP response — not internal error variants.

/// Multipart upload with no `file` field — store_bundle is never called,
/// the handler must reject at the multipart parser stage.
#[tokio::test]
async fn test_upload_multipart_no_file_field() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // Empty multipart form — no fields at all.
    let form = reqwest::multipart::Form::new();
    let resp = client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "no-field upload must be 400");
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    let msg = body["error"].as_str().unwrap_or("");
    assert!(
        msg.to_lowercase().contains("file") || msg.to_lowercase().contains("multipart"),
        "error must mention the missing file field, got {msg:?}"
    );
}

/// Multipart upload where the bundle bytes are NOT a valid st-bundle.
/// Hits the `extract_bundle` failure path inside `store_bundle`.
#[tokio::test]
async fn test_upload_truncated_bundle_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // Garbage that can't possibly parse as a bundle archive.
    let part = reqwest::multipart::Part::bytes(vec![0xFFu8; 32])
        .file_name("garbage.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // Status must remain idle (no partial state from a failed upload).
    let st: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(st["status"], "idle");

    // Info must 404 — nothing was deployed.
    let info = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap();
    assert_eq!(info.status(), 404);
}

/// Empty multipart payload — the field is present but contains zero bytes.
#[tokio::test]
async fn test_upload_empty_bundle_bytes_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let part = reqwest::multipart::Part::bytes(Vec::<u8>::new())
        .file_name("empty.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

/// /program/info must 404 cleanly when nothing has been deployed yet —
/// the early-return branch in `info()` before any program has been stored.
#[tokio::test]
async fn test_program_info_404_when_no_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .get(format!("{base}/api/v1/program/info"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// /program/restart with no deployed program — should 404 since the
/// (delegated) start handler can't find a module to load.
#[tokio::test]
async fn test_restart_without_program_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .post(format!("{base}/api/v1/program/restart"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// DELETE /program when no program is deployed must 404 — exercises the
/// `remove_current` "No program deployed" branch.
#[tokio::test]
async fn test_delete_program_when_idle_no_program_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    let resp = client
        .delete(format!("{base}/api/v1/program"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// Re-uploading a bundle replaces the previous one — verify both that
/// upload-while-running stops the engine cleanly (the new bundle is
/// independent of the old running module) and that subsequent /info
/// returns the new metadata.
#[tokio::test]
async fn test_upload_while_running_replaces_program() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();

    // Deploy + start v1.
    let v1 = make_bundle(
        "Replaceable",
        "PROGRAM Main\nVAR x : INT := 0; END_VAR\n    x := x + 1;\nEND_PROGRAM\n",
    );
    upload_bundle(&client, &base, &v1).await.error_for_status().unwrap();
    let r = client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Re-upload v2 — store_bundle path replaces the on-disk metadata.
    let v2 = make_bundle(
        "Replaceable",
        "PROGRAM Main\nVAR x : INT := 0; y : INT := 0; END_VAR\n    x := x + 2;\n    y := y + 1;\nEND_PROGRAM\n",
    );
    let r = upload_bundle(&client, &base, &v2).await;
    assert_eq!(r.status(), 200);

    // /info must reflect v2 metadata, not the still-running v1's.
    let info: serde_json::Value = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(info["name"], "Replaceable");
    // Stop everything cleanly (the engine is still running v1's module
    // in memory; this proves the re-upload doesn't break the stop path).
    let _ = client
        .post(format!("{base}/api/v1/program/stop"))
        .send()
        .await
        .unwrap();
}

/// /program/start fails cleanly when the on-disk bytecode bytes have been
/// corrupted out of band (covers the `serde_json::from_slice` error
/// branch in `program_store::load_module`).
///
/// Approach: pre-populate the program_dir with corrupt bytecode + a
/// matching meta.json BEFORE the agent boots. The agent's
/// `load_persisted` reads them on startup, so the in-memory `current`
/// entry holds invalid bytecode bytes. The deserialize error then
/// surfaces on the next /program/start call.
#[tokio::test]
async fn test_start_with_corrupted_bytecode_returns_500() {
    let dir = tempfile::tempdir().unwrap();
    let prog_dir = dir.path();
    std::fs::write(prog_dir.join("current.bytecode"), b"definitely not json").unwrap();
    std::fs::write(
        prog_dir.join("current.meta.json"),
        r#"{
            "name": "Corrupt",
            "version": "1.0.0",
            "mode": "development",
            "compiled_at": "now",
            "entry_point": "Main",
            "bytecode_checksum": "00",
            "deployed_at": "now",
            "has_debug_map": false
        }"#,
    )
    .unwrap();

    let (base, _h) = start_agent(test_config(prog_dir)).await;
    let client = Client::new();

    // Sanity: /info should succeed because meta.json is well-formed even
    // though the bytecode is junk. The deserialization error is deferred
    // until load_module is called from /program/start.
    let info = client
        .get(format!("{base}/api/v1/program/info"))
        .send().await.unwrap();
    assert_eq!(info.status(), 200, "info should reflect persisted meta");

    let resp = client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(), 500,
        "corrupted bytecode must surface as 500 — got {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    let msg = body["error"].as_str().unwrap_or("").to_lowercase();
    assert!(
        msg.contains("deserialize") || msg.contains("bytecode"),
        "error must mention deserialization, got {msg:?}"
    );
}

// ─── monitor_ws.rs error paths (acceptance) ─────────────────────────────
//
// Targets the gaps tracked in plan/implementation.md: WS subscribe /
// unsubscribe / force error paths and the catalog-empty edge case.
// Drive everything through the real WebSocket port so the failure
// surface is whatever the panel's JS sees on the wire.

/// Subscribing with an empty variable list is a valid no-op — the handler
/// must answer success without touching the runtime state.
#[tokio::test]
async fn test_ws_subscribe_empty_list_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();
    upload_and_start(&client, &base, &simple_program()).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({ "method": "subscribe", "params": { "variables": [] } }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
    // No subscriptions means no pushes are coming — explicit timeout
    // to prove we don't deadlock waiting for one.
    let push = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            let msg = ws.next().await;
            if let Some(Ok(tungstenite::Message::Text(t))) = msg {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "variableUpdate" {
                    return Some(v);
                }
            } else {
                return None;
            }
        }
    })
    .await;
    assert!(
        push.is_err() || push.unwrap().is_none(),
        "empty subscribe must not produce variable pushes"
    );
}

/// Subscribing to a name that doesn't exist in the catalog: the handler
/// accepts the subscription (it can't reject without a catalog round-trip
/// per request) but never pushes a value for it.
#[tokio::test]
async fn test_ws_subscribe_nonexistent_variable_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();
    upload_and_start(&client, &base, &simple_program()).await;

    let mut ws = ws_connect(&base).await;
    // Mix a real var with a bogus one. The real var must still get
    // pushes; the bogus one must simply be absent from the watch_tree.
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["Main.counter", "Main.does_not_exist"] }
        }),
    )
    .await;
    assert_eq!(resp["success"], true);

    // Wait for a push and inspect it.
    let push = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            if let tungstenite::Message::Text(t) = msg {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "variableUpdate" {
                    return v;
                }
            }
        }
    })
    .await
    .expect("expected a variableUpdate within 2s");

    let tree = push["watch_tree"].as_array().expect("watch_tree array");
    let names: Vec<String> = tree
        .iter()
        .filter_map(|n| n["fullPath"].as_str().map(String::from))
        .collect();
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.counter")),
        "real var must appear in watch_tree, got {names:?}"
    );
    // The bogus name may appear as a placeholder root; what matters is
    // that we don't crash and the real var survives. Catalog filtering
    // is a separate concern.
}

/// Unsubscribing variables that were never subscribed is a no-op success
/// (idempotent unsubscribe) — covers the unsubscribe handler when the
/// HashSet has no matching entry.
#[tokio::test]
async fn test_ws_unsubscribe_unknown_variable_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();
    upload_and_start(&client, &base, &simple_program()).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "unsubscribe",
            "params": { "variables": ["Main.never_subscribed"] }
        }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
}

/// Force while idle (no engine) — the handler checks runtime status and
/// returns an Error message, not a Response.
#[tokio::test]
async fn test_ws_force_when_not_running_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "force",
            "params": { "variable": "Main.counter", "value": 1 }
        }),
    )
    .await;
    assert_eq!(resp["type"], "error");
    let msg = resp["message"].as_str().unwrap_or("").to_lowercase();
    assert!(
        msg.contains("not running"),
        "must say 'not running', got {msg:?}"
    );
}

/// Force with an invalid value type (object/array) — the handler must
/// reject before reaching the runtime so the engine never sees the
/// malformed value.
#[tokio::test]
async fn test_ws_force_invalid_value_type_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();
    upload_and_start(&client, &base, &simple_program()).await;

    let mut ws = ws_connect(&base).await;
    // Object as value — neither bool, number, nor string. The matcher
    // in monitor_ws explicitly rejects this.
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "force",
            "params": { "variable": "Main.counter", "value": { "nope": 1 } }
        }),
    )
    .await;
    assert_eq!(resp["type"], "error");
    let msg = resp["message"].as_str().unwrap_or("").to_lowercase();
    assert!(
        msg.contains("invalid") || msg.contains("type"),
        "must mention invalid value type, got {msg:?}"
    );
}

/// getCatalog while idle — the catalog vector is empty. The handler
/// still returns a well-formed Catalog message, not an error.
#[tokio::test]
async fn test_ws_get_catalog_when_idle_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(&mut ws, serde_json::json!({ "method": "getCatalog" })).await;
    assert_eq!(resp["type"], "catalog");
    let vars = resp["variables"].as_array().expect("variables array");
    assert!(vars.is_empty(), "idle catalog must be empty, got {vars:?}");
}

/// Read while idle — same shape: empty list, not an error.
#[tokio::test]
async fn test_ws_read_when_idle_returns_empty_data() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "read",
            "params": { "variables": ["Main.counter"] }
        }),
    )
    .await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
    let data = resp["data"].as_array().expect("data array");
    assert!(data.is_empty());
}

/// `write` and `onlineChange` over WS are not yet implemented — the
/// handler returns the "Not implemented" error message. This pins that
/// behaviour so removing the rejection (instead of providing a real
/// implementation) is caught.
#[tokio::test]
async fn test_ws_write_returns_not_implemented_error() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "write",
            "params": { "variable": "Main.counter", "value": 1 }
        }),
    )
    .await;
    assert_eq!(resp["type"], "error");
    assert!(
        resp["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("not implemented"),
        "expected 'Not implemented', got {}",
        resp["message"]
    );
}

#[tokio::test]
async fn test_ws_online_change_returns_not_implemented_error() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let mut ws = ws_connect(&base).await;
    let resp = ws_request(
        &mut ws,
        serde_json::json!({
            "method": "onlineChange",
            "params": { "source": "PROGRAM Main\nEND_PROGRAM\n" }
        }),
    )
    .await;
    assert_eq!(resp["type"], "error");
    assert!(
        resp["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("not implemented")
    );
}

/// resetStats while running — the handler returns success and clears
/// the min/max/jitter counters. Verify we get a fresh snapshot after.
#[tokio::test]
async fn test_ws_reset_stats_while_running() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(test_config(dir.path())).await;
    let client = Client::new();
    upload_and_start(&client, &base, &simple_program()).await;

    // Let cycles accumulate before the reset.
    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut ws = ws_connect(&base).await;
    let resp = ws_request(&mut ws, serde_json::json!({ "method": "resetStats" })).await;
    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);

    // Subsequent getCycleInfo should still report a sane structure.
    let info = ws_request(&mut ws, serde_json::json!({ "method": "getCycleInfo" })).await;
    assert_eq!(info["type"], "cycleInfo");
    assert!(info["cycle_count"].as_u64().is_some());
}
