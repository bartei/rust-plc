//! Integration tests for the DAP proxy.
//!
//! Tests the full flow: start agent, upload development bundle, connect to
//! DAP proxy TCP port, exercise DAP protocol (Initialize → Launch → Stopped →
//! Variables → Step → Continue → Disconnect).

use reqwest::Client;
use st_deploy::bundle::{create_bundle, write_bundle, BundleOptions};
use st_target_agent::config::AgentConfig;
use st_target_agent::server::{build_app_state, build_router};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tokio::net::TcpListener;

/// Start agent + DAP proxy on random ports. Returns (http_base_url, dap_port).
async fn start_agent_with_dap(
    config: AgentConfig,
) -> (String, u16, tokio::task::JoinHandle<()>) {
    let state = build_app_state(config, None).unwrap();
    let router = build_router(state.clone());

    // HTTP server on random port
    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_addr = http_listener.local_addr().unwrap();
    let http_handle = tokio::spawn(async move {
        axum::serve(http_listener, router).await.unwrap();
    });

    // DAP proxy on random port (pre-bound to avoid race)
    let dap_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dap_port = dap_listener.local_addr().unwrap().port();

    // Find st-cli binary
    let st_cli = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/st-cli");
    assert!(st_cli.exists(), "st-cli not found at {}. Run `cargo build -p st-cli` first.", st_cli.display());

    let dap_state = state.clone();
    tokio::spawn(st_target_agent::dap_proxy::run_dap_proxy_with_listener(
        dap_listener,
        dap_state,
        st_cli,
    ));

    // Brief pause for servers to start accepting
    tokio::time::sleep(Duration::from_millis(500)).await;

    eprintln!("[TEST] Agent HTTP: http://{http_addr}, DAP port: {dap_port}");

    (format!("http://{http_addr}"), dap_port, http_handle)
}

fn test_config(dir: &std::path::Path) -> AgentConfig {
    let mut config = AgentConfig::default();
    config.storage.program_dir = dir.to_path_buf();
    config
}

fn make_debug_bundle() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        "name: DapTest\nversion: '1.0.0'\nentryPoint: Main\n",
    )
    .unwrap();
    fs::write(
        root.join("main.st"),
        concat!(
            "PROGRAM Main\n",
            "VAR\n",
            "    counter : INT := 0;\n",
            "    flag : BOOL := FALSE;\n",
            "END_VAR\n",
            "    counter := counter + 1;\n",
            "    flag := counter > 5;\n",
            "END_PROGRAM\n",
        ),
    )
    .unwrap();

    let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
    let path = root.join("test.st-bundle");
    write_bundle(&bundle, &path).unwrap();
    fs::read(&path).unwrap()
}

async fn upload_bundle(client: &Client, base: &str, data: &[u8]) {
    let part = reqwest::multipart::Part::bytes(data.to_vec()).file_name("program.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Upload failed");
}

// ── DAP wire protocol helpers ───────────────────────────────────────────

fn send_dap_request(stream: &mut TcpStream, seq: i64, command: &str, args: serde_json::Value) {
    let msg = if args.is_null() {
        serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command
        })
    } else {
        serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args
        })
    };
    let json = serde_json::to_string(&msg).unwrap();
    let framed = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
    stream.write_all(framed.as_bytes()).unwrap();
    stream.flush().unwrap();
}

#[allow(dead_code)]
fn read_dap_message(reader: &mut BufReader<&TcpStream>) -> serde_json::Value {
    // Read Content-Length header
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().unwrap();
        }
    }
    assert!(content_length > 0, "Missing Content-Length header");

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Read messages until we find one matching the predicate.
fn read_until(
    reader: &mut BufReader<&TcpStream>,
    predicate: impl Fn(&serde_json::Value) -> bool,
    timeout_ms: u64,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    reader.get_ref().set_read_timeout(Some(Duration::from_millis(500))).unwrap();

    loop {
        if std::time::Instant::now() > deadline {
            panic!("Timeout waiting for DAP message matching predicate");
        }
        match read_dap_message_timeout(reader) {
            Some(msg) => {
                if predicate(&msg) {
                    reader.get_ref().set_read_timeout(None).unwrap();
                    return msg;
                }
                // Not what we're looking for, continue reading
            }
            None => {
                // Timeout on this read, try again
                continue;
            }
        }
    }
}

fn read_dap_message_timeout(reader: &mut BufReader<&TcpStream>) -> Option<serde_json::Value> {
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None, // EOF
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return None,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return None,
            Err(e) => panic!("Read error: {e}"),
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().unwrap();
        }
    }
    if content_length == 0 {
        return None;
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_proxy_initialize_and_launch() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // Upload a development bundle (with source)
    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;

    // Give a moment for source extraction
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Run the DAP protocol exchange in a blocking task to avoid starving
    // the tokio runtime (DAP uses blocking reads with Content-Length framing).
    let dap_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}"))
            .expect("Cannot connect to DAP proxy");
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();

        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        // 1. Initialize
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({
            "adapterID": "st",
            "clientID": "test",
            "clientName": "Integration Test"
        }));

        let init_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "initialize"
        }, 15000);
        assert_eq!(init_resp["success"], true, "Initialize should succeed: {init_resp}");

        // 2. Launch
        send_dap_request(&mut writer, 2, "launch", serde_json::json!({
            "stopOnEntry": true
        }));

        let launch_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "launch"
        }, 15000);
        assert_eq!(launch_resp["success"], true, "Launch should succeed: {launch_resp}");

        // After Launch with stopOnEntry, the server sends:
        //   Output events → Stopped(entry) → varCatalog → Initialized
        // We need to read Stopped FIRST (it comes before Initialized)
        let stopped = read_until(&mut reader, |m| {
            m["type"] == "event" && m["event"] == "stopped"
        }, 15000);
        assert_eq!(stopped["body"]["reason"], "entry", "Should stop on entry: {stopped}");

        // Initialized event
        let _init_event = read_until(&mut reader, |m| {
            m["type"] == "event" && m["event"] == "initialized"
        }, 5000);

        // 3. ConfigurationDone
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);

        // ConfigurationDone response
        let _config_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "configurationDone"
        }, 5000);

        // 4. StackTrace
        send_dap_request(&mut writer, 4, "stackTrace", serde_json::json!({
            "threadId": 1, "startFrame": 0, "levels": 10
        }));

        let stack_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "stackTrace"
        }, 5000);
        assert_eq!(stack_resp["success"], true);
        let frames = stack_resp["body"]["stackFrames"].as_array().unwrap();
        assert!(!frames.is_empty(), "Should have stack frames");
        assert!(
            frames[0]["name"].as_str().unwrap().contains("Main"),
            "Top frame should be Main: {:?}", frames[0]
        );

        // 5. Scopes
        let frame_id = frames[0]["id"].as_i64().unwrap();
        send_dap_request(&mut writer, 5, "scopes", serde_json::json!({
            "frameId": frame_id
        }));

        let scopes_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "scopes"
        }, 5000);
        assert_eq!(scopes_resp["success"], true);
        let scopes = scopes_resp["body"]["scopes"].as_array().unwrap();

        // 6. Variables (Locals)
        let locals_ref = scopes.iter()
            .find(|s| s["name"] == "Locals")
            .unwrap()["variablesReference"]
            .as_i64().unwrap();

        send_dap_request(&mut writer, 6, "variables", serde_json::json!({
            "variablesReference": locals_ref
        }));

        let vars_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "variables"
        }, 5000);
        assert_eq!(vars_resp["success"], true);
        let variables = vars_resp["body"]["variables"].as_array().unwrap();
        assert!(!variables.is_empty(), "Should have local variables");

        let counter_var = variables.iter().find(|v| {
            v["name"].as_str().map(|n| n.eq_ignore_ascii_case("counter")).unwrap_or(false)
        });
        assert!(counter_var.is_some(), "Should find 'counter' variable: {variables:?}");

        // 7. Step (next)
        send_dap_request(&mut writer, 7, "next", serde_json::json!({
            "threadId": 1
        }));

        let stepped = read_until(&mut reader, |m| {
            m["type"] == "event" && m["event"] == "stopped"
        }, 15000);
        assert!(
            stepped["body"]["reason"] == "step" || stepped["body"]["reason"] == "entry",
            "Should stop after step: {stepped}"
        );

        // 8. Disconnect
        send_dap_request(&mut writer, 8, "disconnect", serde_json::json!({
            "terminateDebuggee": true
        }));

        eprintln!("[TEST] DAP protocol exchange completed successfully");
    })
    .await;

    assert!(dap_result.is_ok(), "DAP protocol exchange failed: {:?}", dap_result.err());
}

#[tokio::test]
async fn test_dap_proxy_rejects_release_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // Upload a RELEASE bundle (no source)
    let bundle_dir = tempfile::tempdir().unwrap();
    let root = bundle_dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        "name: ReleaseTest\nversion: '1.0.0'\nentryPoint: Main\n",
    )
    .unwrap();
    fs::write(
        root.join("main.st"),
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    )
    .unwrap();

    let bundle = create_bundle(
        root,
        &st_deploy::bundle::BundleOptions {
            mode: st_deploy::BundleMode::Release,
            ..Default::default()
        },
    )
    .unwrap();
    let path = root.join("release.st-bundle");
    write_bundle(&bundle, &path).unwrap();
    let data = fs::read(&path).unwrap();

    // Upload release bundle
    let part = reqwest::multipart::Part::bytes(data).file_name("program.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Try to connect to DAP proxy — should fail/be rejected
    // (release bundles have no source, agent rejects the connection)
    let result = TcpStream::connect_timeout(
        &format!("127.0.0.1:{dap_port}").parse().unwrap(),
        Duration::from_secs(2),
    );

    match result {
        Ok(stream) => {
            // Connection accepted but should close immediately
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 1];
            // The agent drops the connection for release bundles — read should fail or return 0
            let n = stream.peek(&mut buf).unwrap_or(0);
            // If we get data, the agent didn't reject — that's a bug
            // But if connection was accepted then immediately closed, peek returns 0
            assert_eq!(n, 0, "DAP proxy should reject release bundle connections");
        }
        Err(_) => {
            // Connection refused — this is also acceptable
        }
    }
}

#[tokio::test]
async fn test_dap_proxy_no_program_rejects() {
    let dir = tempfile::tempdir().unwrap();
    let (_base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;

    // No program uploaded — DAP connection should be rejected
    tokio::time::sleep(Duration::from_millis(200)).await;

    let result = TcpStream::connect_timeout(
        &format!("127.0.0.1:{dap_port}").parse().unwrap(),
        Duration::from_secs(2),
    );

    match result {
        Ok(stream) => {
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 1];
            let n = stream.peek(&mut buf).unwrap_or(0);
            assert_eq!(n, 0, "DAP proxy should reject when no program deployed");
        }
        Err(_) => {
            // Connection refused — acceptable
        }
    }
}

/// Test that DAP attach to a RUNNING engine works without stopping execution.
/// This replicates the VS Code attach flow exactly:
/// Initialize → Attach → ConfigurationDone → verify session stays alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_to_running_engine() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // 1. Upload a development bundle
    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 2. Start the program via API
    let start_resp = client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    assert_eq!(start_resp.status(), 200, "Start should succeed");

    // 3. Let it run a few cycles
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify it's actually running
    let status_resp: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_resp["status"], "running", "Program should be running: {status_resp}");
    let cycle_count_before = status_resp["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycle_count_before > 0, "Should have run some cycles: {status_resp}");

    // 4. Connect to DAP port and do the attach protocol
    let dap_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}"))
            .expect("Cannot connect to DAP proxy");
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();

        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        // 4a. Initialize
        eprintln!("[TEST] Sending initialize...");
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({
            "adapterID": "st",
            "clientID": "test",
            "clientName": "AttachTest",
            "supportsRunInTerminalRequest": false
        }));

        let init_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "initialize"
        }, 10000);
        assert_eq!(init_resp["success"], true, "Initialize failed: {init_resp}");
        eprintln!("[TEST] Initialize OK: {init_resp}");

        // 4b. Attach (NOT launch)
        eprintln!("[TEST] Sending attach...");
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({
            "target": "test",
            "stopOnEntry": false
        }));

        let attach_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "attach"
        }, 10000);
        assert_eq!(attach_resp["success"], true, "Attach failed: {attach_resp}");
        eprintln!("[TEST] Attach OK");

        // 4c. Should receive "initialized" event
        let init_event = read_until(&mut reader, |m| {
            m["type"] == "event" && m["event"] == "initialized"
        }, 5000);
        eprintln!("[TEST] Got initialized event: {init_event}");

        // 4d. ConfigurationDone
        eprintln!("[TEST] Sending configurationDone...");
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);

        let config_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "configurationDone"
        }, 5000);
        assert_eq!(config_resp["success"], true, "ConfigurationDone failed: {config_resp}");
        eprintln!("[TEST] ConfigurationDone OK");

        // 4e. We should NOT receive a "stopped" event — engine keeps running.
        // Wait 2 seconds and verify no stopped event arrives.
        eprintln!("[TEST] Verifying no stopped event (engine should keep running)...");
        writer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let stopped = read_dap_message_timeout(&mut reader);
        if let Some(ref msg) = stopped {
            if msg["type"] == "event" && msg["event"] == "stopped" {
                panic!("Engine should NOT stop on attach! Got: {msg}");
            }
            eprintln!("[TEST] Got non-stopped message (OK): {msg}");
        } else {
            eprintln!("[TEST] No stopped event — engine is running (correct!)");
        }

        // 4f. Send threads request to verify session is alive
        writer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        send_dap_request(&mut writer, 4, "threads", serde_json::Value::Null);
        let threads_resp = read_until(&mut reader, |m| {
            m["type"] == "response" && m["command"] == "threads"
        }, 5000);
        assert_eq!(threads_resp["success"], true, "Threads failed: {threads_resp}");
        eprintln!("[TEST] Threads OK — session is alive");

        // 4g. Disconnect
        send_dap_request(&mut writer, 5, "disconnect", serde_json::json!({
            "terminateDebuggee": false
        }));
        eprintln!("[TEST] Disconnect sent");
    })
    .await;

    assert!(dap_result.is_ok(), "DAP attach protocol failed: {:?}", dap_result.err());

    // 5. Verify the program is STILL running after disconnect
    tokio::time::sleep(Duration::from_millis(500)).await;
    let status_after: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_after["status"], "running", "Program should still be running after debug disconnect: {status_after}");
    let cycle_count_after = status_after["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycle_count_after > cycle_count_before, "Cycles should advance after disconnect: before={cycle_count_before}, after={cycle_count_after}");

    eprintln!("[TEST] Program still running with {cycle_count_after} cycles (was {cycle_count_before} before attach)");
}

/// Comprehensive test: attach → pause → inspect variables → resume → pause again
/// → disconnect → verify engine resumes → re-attach → verify engine still works.
/// This catches stateful issues where the engine gets stuck after debug sessions.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_pause_resume_reattach_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // Upload and start
    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start_resp = client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    assert_eq!(start_resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify running
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running", "Should be running");
    let initial_cycles = status["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(initial_cycles > 0, "Should have cycles");
    eprintln!("[TEST] Initial cycles: {initial_cycles}");

    // === SESSION 1: Attach → Pause → Variables → Resume → Disconnect ===
    let session1_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        // Initialize + Attach
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);
        eprintln!("[SESSION1] Attached, engine running");

        // Pause
        send_dap_request(&mut writer, 4, "pause", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "pause", 5000);
        // Wait for stopped event
        let stopped = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 10000);
        eprintln!("[SESSION1] Paused: reason={}", stopped["body"]["reason"]);

        // Get variables while paused
        send_dap_request(&mut writer, 5, "scopes", serde_json::json!({"frameId": 0}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "scopes", 5000);
        send_dap_request(&mut writer, 6, "variables", serde_json::json!({"variablesReference": 1000}));
        let vars_resp = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "variables", 5000);
        let vars = vars_resp["body"]["variables"].as_array();
        eprintln!("[SESSION1] Variables: {} items", vars.map(|v| v.len()).unwrap_or(0));

        // Continue
        send_dap_request(&mut writer, 7, "continue", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "continue", 5000);
        eprintln!("[SESSION1] Resumed");

        // Wait a moment for some cycles to run
        std::thread::sleep(Duration::from_millis(500));

        // Disconnect
        send_dap_request(&mut writer, 8, "disconnect", serde_json::json!({"terminateDebuggee": false}));
        eprintln!("[SESSION1] Disconnected");
    }).await;
    assert!(session1_result.is_ok(), "Session 1 failed: {:?}", session1_result.err());

    // === VERIFY: Engine resumes after disconnect ===
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running", "Engine should resume after disconnect, got: {}", status["status"]);
    let cycles_after_s1 = status["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycles_after_s1 > initial_cycles, "Cycles should advance after session 1: was {initial_cycles}, now {cycles_after_s1}");
    eprintln!("[TEST] After session 1: status={}, cycles={cycles_after_s1}", status["status"]);

    // === SESSION 2: Re-attach to verify engine isn't corrupted ===
    let session2_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        // Initialize + Attach
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);
        eprintln!("[SESSION2] Re-attached successfully");

        // Pause again
        send_dap_request(&mut writer, 4, "pause", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "pause", 5000);
        let stopped = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 10000);
        eprintln!("[SESSION2] Paused: reason={}", stopped["body"]["reason"]);

        // Disconnect without resuming — engine should auto-resume
        send_dap_request(&mut writer, 5, "disconnect", serde_json::json!({"terminateDebuggee": false}));
        eprintln!("[SESSION2] Disconnected while paused");
    }).await;
    assert!(session2_result.is_ok(), "Session 2 failed: {:?}", session2_result.err());

    // === VERIFY: Engine resumes after session 2 (disconnected while paused) ===
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running", "Engine should resume after paused disconnect, got: {}", status["status"]);
    let cycles_after_s2 = status["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycles_after_s2 > cycles_after_s1, "Cycles should advance after session 2: was {cycles_after_s1}, now {cycles_after_s2}");
    eprintln!("[TEST] After session 2: status={}, cycles={cycles_after_s2}", status["status"]);

    // === VERIFY: Stop works from running state ===
    let stop_resp = client.post(format!("{base}/api/v1/program/stop")).send().await.unwrap();
    assert_eq!(stop_resp.status(), 200, "Stop should succeed");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "idle", "Should be idle after stop");
    eprintln!("[TEST] PASS — full lifecycle: attach → pause → resume → disconnect → re-attach → pause → disconnect → stop");
}
