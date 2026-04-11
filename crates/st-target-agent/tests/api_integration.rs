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
