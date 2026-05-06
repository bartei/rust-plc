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

/// Test that only one debug session can be active at a time (offline/idle engine path).
/// A second connection is rejected while the first is active, and a third
/// connection succeeds after the first disconnects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_proxy_rejects_second_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // Upload debug bundle (engine stays idle — offline debug path)
    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Client 1: connect and send Initialize to confirm session is active.
    // Keep the stream alive across client 2's rejection test by returning it.
    let dap_port_for_c1 = dap_port;
    let client1_stream = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect_timeout(
            &format!("127.0.0.1:{dap_port_for_c1}").parse().unwrap(),
            Duration::from_secs(5),
        )
        .expect("Client 1 should connect");
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let init_resp = read_until(
            &mut reader,
            |m| m["type"] == "response" && m["command"] == "initialize",
            15000,
        );
        assert!(init_resp["success"].as_bool().unwrap_or(false), "Client 1 Initialize should succeed");
        eprintln!("[CLIENT1] Connected and initialized");
        writer // return to keep session alive
    }).await.unwrap();

    // Brief pause to ensure session is registered
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Client 2: should be rejected (connection accepted then immediately closed)
    let result2 = TcpStream::connect_timeout(
        &format!("127.0.0.1:{dap_port}").parse().unwrap(),
        Duration::from_secs(2),
    );
    match result2 {
        Ok(stream2) => {
            stream2.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 1];
            let n = stream2.peek(&mut buf).unwrap_or(0);
            assert_eq!(n, 0, "Client 2 should be rejected (session already active)");
            eprintln!("[TEST] Client 2 correctly rejected");
        }
        Err(_) => {
            eprintln!("[TEST] Client 2 connection refused (also acceptable)");
        }
    }

    // Disconnect client 1 by dropping it
    drop(client1_stream);
    eprintln!("[TEST] Client 1 disconnected");

    // Wait for session cleanup (subprocess needs to terminate)
    tokio::time::sleep(Duration::from_millis(3000)).await;

    // Client 3: should succeed now that session is cleared
    // Use spawn_blocking because the offline DAP subprocess bridge may take time to start
    let dap_port_for_c3 = dap_port;
    let client3_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect_timeout(
            &format!("127.0.0.1:{dap_port_for_c3}").parse().unwrap(),
            Duration::from_secs(5),
        )
        .expect("Client 3 should connect after session cleared");
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let init_resp = read_until(
            &mut reader,
            |m| m["type"] == "response" && m["command"] == "initialize",
            15000,
        );
        assert!(init_resp["success"].as_bool().unwrap_or(false), "Client 3 Initialize should succeed");
        eprintln!("[CLIENT3] Connected and initialized after session 1 ended");
        send_dap_request(&mut writer, 2, "disconnect", serde_json::Value::Null);
    }).await;
    assert!(client3_result.is_ok(), "Client 3 failed: {:?}", client3_result.err());
    eprintln!("[TEST] PASS — single-session enforcement: client 2 rejected, client 3 accepted after disconnect");
}

/// Test single-session enforcement on the attach path (running engine).
/// A second client is rejected while the first is attached, and a third
/// client can attach after the first disconnects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_rejects_second_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _handle) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    // Upload and start program (engine Running — attach path)
    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start_resp = client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    assert_eq!(start_resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify running
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running");

    // Client 1: attach successfully
    let dap_port_for_s1 = dap_port;
    let session1 = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port_for_s1}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        // Initialize + Attach
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let init = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        assert!(init["success"].as_bool().unwrap_or(false));
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);
        eprintln!("[SESSION1] Attached to running engine");

        // Return the writer to keep the session alive
        writer
    }).await.unwrap();
    eprintln!("[TEST] Client 1 attached");

    // Brief pause to ensure session is registered
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Client 2: should be rejected
    let result2 = TcpStream::connect_timeout(
        &format!("127.0.0.1:{dap_port}").parse().unwrap(),
        Duration::from_secs(2),
    );
    match result2 {
        Ok(stream2) => {
            stream2.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 1];
            let n = stream2.peek(&mut buf).unwrap_or(0);
            assert_eq!(n, 0, "Client 2 should be rejected (attach session active)");
            eprintln!("[TEST] Client 2 correctly rejected");
        }
        Err(_) => {
            eprintln!("[TEST] Client 2 connection refused (also acceptable)");
        }
    }

    // Disconnect client 1 by sending disconnect and dropping
    tokio::task::spawn_blocking(move || {
        let mut writer = session1;
        send_dap_request(&mut writer, 10, "disconnect", serde_json::json!({"terminateDebuggee": false}));
        eprintln!("[TEST] Client 1 sent disconnect");
        // Drop writer to close TCP connection
    }).await.unwrap();

    // Wait for session cleanup
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Client 3: should succeed now
    let dap_port_for_s3 = dap_port;
    let session3_result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port_for_s3}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID": "st"}));
        let init = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        assert!(init["success"].as_bool().unwrap_or(false), "Client 3 Initialize should succeed");
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        eprintln!("[SESSION3] Attached successfully after session 1 ended");

        send_dap_request(&mut writer, 3, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    }).await;
    assert!(session3_result.is_ok(), "Client 3 session failed: {:?}", session3_result.err());

    eprintln!("[TEST] PASS — attach single-session: client 2 rejected, client 3 accepted after disconnect");
}

// ── dap_attach_handler.rs deeper coverage ───────────────────────────────
//
// Targets the gaps tracked in plan/implementation.md: disconnect path,
// stackTrace edge cases, variables for FB fields, breakpoint resolution
// at virtual_offset (multi-file). Each test drives the agent's TCP DAP
// proxy port so the attach_handler receives real wire traffic and the
// engine's run-loop is the live target — no mocks.

/// Build a bundle with an FB instance — exercises Variables(fb_ref) →
/// FB-field expansion in the attach handler, and gives stackTrace
/// something to resolve to.
fn make_fb_debug_bundle() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        "name: DapAttachFb\nversion: '1.0.0'\nentryPoint: Main\n",
    )
    .unwrap();
    fs::write(
        root.join("main.st"),
        concat!(
            "FUNCTION_BLOCK Counter\n",
            "VAR_INPUT\n",
            "    cu : BOOL;\n",
            "END_VAR\n",
            "VAR\n",
            "    cv : INT := 0;\n",
            "END_VAR\n",
            "    IF cu THEN\n",
            "        cv := cv + 1;\n",
            "    END_IF;\n",
            "END_FUNCTION_BLOCK\n",
            "\n",
            "PROGRAM Main\n",
            "VAR\n",
            "    counter : INT := 0;\n",
            "    fb : Counter;\n",
            "    flag : BOOL := FALSE;\n",
            "END_VAR\n",
            "    counter := counter + 1;\n",
            "    flag := counter > 5;\n",
            "    fb(cu := flag);\n",
            "END_PROGRAM\n",
        ),
    )
    .unwrap();
    let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
    let path = root.join("test.st-bundle");
    write_bundle(&bundle, &path).unwrap();
    fs::read(&path).unwrap()
}

/// Multi-file project: a helper FB lives in a separate `helper.st`. We
/// need this to drive `setBreakpoints` with a non-zero `source_offset`
/// (the helper's content is concatenated AFTER main.st in the virtual
/// compilation buffer). The attach handler must resolve the breakpoint
/// against the per-file virtual offset, not the file's own line count.
fn make_multifile_debug_bundle() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("plc-project.yaml"),
        "name: DapAttachMultiFile\nversion: '1.0.0'\nentryPoint: Main\n",
    )
    .unwrap();
    fs::write(
        root.join("helper.st"),
        concat!(
            "FUNCTION_BLOCK HelperFb\n",
            "VAR_INPUT\n",
            "    inc : INT;\n",
            "END_VAR\n",
            "VAR\n",
            "    total : INT := 0;\n",
            "END_VAR\n",
            "    total := total + inc;\n",
            "END_FUNCTION_BLOCK\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("main.st"),
        concat!(
            "PROGRAM Main\n",
            "VAR\n",
            "    counter : INT := 0;\n",
            "    helper : HelperFb;\n",
            "END_VAR\n",
            "    counter := counter + 1;\n",
            "    helper(inc := counter);\n",
            "END_PROGRAM\n",
        ),
    )
    .unwrap();
    let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
    let path = root.join("test.st-bundle");
    write_bundle(&bundle, &path).unwrap();
    fs::read(&path).unwrap()
}

/// TCP-disconnect mid-session must release the engine. The VS Code
/// extension always sends a Disconnect request, but the systemd-managed
/// CLI proxy can drop the TCP connection abruptly (Ctrl+C, network
/// hiccup). The attach handler's auto-detach-on-channel-close path must
/// trigger so the engine resumes normal cycling.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_tcp_drop_releases_engine() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    let bundle = make_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let cycles_before = {
        let s: serde_json::Value = client
            .get(format!("{base}/api/v1/status"))
            .send().await.unwrap().json().await.unwrap();
        s["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0)
    };
    assert!(cycles_before > 0);

    // Attach, then DROP the TCP connection without sending Disconnect.
    let dropped = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

        // No Disconnect. Just drop the writer + reader — TCP FIN.
    })
    .await;
    assert!(dropped.is_ok());

    // Give the agent time to notice the channel close and detach.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Engine must still be running and advancing cycles.
    let s: serde_json::Value = client
        .get(format!("{base}/api/v1/status"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(s["status"], "running", "engine must keep running after TCP drop: {s}");
    let cycles_after = s["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(
        cycles_after > cycles_before,
        "engine cycles must advance after TCP drop, before={cycles_before} after={cycles_after}"
    );

    // And a fresh attach must succeed — proves the auto-detach actually
    // released the single-session lock the attach handler holds.
    let dap_port2 = dap_port;
    let reattached = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port2}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let attach = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        assert_eq!(attach["success"], true, "re-attach must succeed: {attach}");
        send_dap_request(&mut writer, 3, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    }).await;
    assert!(reattached.is_ok());
}

/// stackTrace edge case: pause inside a function block call so the call
/// stack has more than one frame. The attach handler must build frames
/// for both `Main` (caller) and `Counter` (callee) with names + ids.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_stacktrace_inside_fb_body() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    let bundle = make_fb_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

        // Pause request — no breakpoint needed; pause halts at the next
        // instruction, which is somewhere in main.st's body.
        send_dap_request(&mut writer, 4, "pause", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "pause", 5000);
        let stopped = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 5000);
        assert!(stopped["body"]["reason"].as_str().is_some(), "stopped event must carry a reason: {stopped}");

        // stackTrace must succeed and return at least one frame named Main.
        send_dap_request(&mut writer, 5, "stackTrace", serde_json::json!({
            "threadId": 1, "startFrame": 0, "levels": 20
        }));
        let st = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "stackTrace", 5000);
        assert_eq!(st["success"], true, "stackTrace must succeed: {st}");
        let frames = st["body"]["stackFrames"].as_array().expect("stackFrames array");
        assert!(!frames.is_empty(), "must have at least one frame");
        let names: Vec<&str> = frames.iter().filter_map(|f| f["name"].as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("Main")),
            "stack must include Main frame, got {names:?}"
        );
        // Each frame carries an i64 id — the attach handler builds these
        // from the call stack indices.
        for f in frames {
            assert!(f["id"].as_i64().is_some(), "frame missing id: {f}");
        }

        send_dap_request(&mut writer, 6, "continue", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "continue", 5000);
        send_dap_request(&mut writer, 7, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    })
    .await;
    assert!(result.is_ok(), "stackTrace edge-case test failed: {:?}", result.err());
}

/// Variables for FB fields: pause the engine, expand the `fb` local —
/// the attach handler must hand back the FB instance's fields (cu, cv).
/// This exercises the Variables(fb_ref_id) → FB field enumeration path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_variables_for_fb_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    let bundle = make_fb_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        send_dap_request(&mut writer, 3, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

        send_dap_request(&mut writer, 4, "pause", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "pause", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 5000);

        // stackTrace → scopes → variables(locals)
        send_dap_request(&mut writer, 5, "stackTrace", serde_json::json!({
            "threadId": 1, "startFrame": 0, "levels": 1
        }));
        let st = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "stackTrace", 5000);
        let frame_id = st["body"]["stackFrames"][0]["id"].as_i64().unwrap();

        send_dap_request(&mut writer, 6, "scopes", serde_json::json!({"frameId": frame_id}));
        let scopes = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "scopes", 5000);
        let locals_ref = scopes["body"]["scopes"].as_array().unwrap()
            .iter().find(|s| s["name"] == "Locals").unwrap()["variablesReference"].as_i64().unwrap();

        send_dap_request(&mut writer, 7, "variables", serde_json::json!({"variablesReference": locals_ref}));
        let locals = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "variables", 5000);
        let vars = locals["body"]["variables"].as_array().expect("locals array");

        // The `fb` local must appear with an expandable variablesReference > 0.
        let fb_var = vars.iter()
            .find(|v| v["name"].as_str().map(|s| s.eq_ignore_ascii_case("fb")).unwrap_or(false))
            .unwrap_or_else(|| panic!("fb local missing from {vars:?}"));
        let fb_ref = fb_var["variablesReference"].as_i64().unwrap_or(0);
        assert!(fb_ref > 0, "FB instance must be expandable, got ref={fb_ref}");

        // Expand it — the response must list cu and cv with concrete values.
        send_dap_request(&mut writer, 8, "variables", serde_json::json!({"variablesReference": fb_ref}));
        let fb_fields = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "variables", 5000);
        assert_eq!(fb_fields["success"], true, "variables(fb_ref) must succeed: {fb_fields}");
        let fields = fb_fields["body"]["variables"].as_array().expect("fb field array");
        let names: Vec<String> = fields.iter()
            .filter_map(|f| f["name"].as_str().map(String::from))
            .collect();
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case("cv")),
            "FB fields must include cv, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case("cu")),
            "FB fields must include cu, got {names:?}"
        );
        // Field rows must carry concrete values, not "?" placeholders.
        let cv = fields.iter().find(|f| f["name"].as_str().map(|n| n.eq_ignore_ascii_case("cv")).unwrap_or(false)).unwrap();
        let cv_val = cv["value"].as_str().unwrap_or("");
        assert!(cv_val.parse::<i64>().is_ok(), "cv must be a numeric string, got {cv_val:?}");

        send_dap_request(&mut writer, 9, "continue", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "continue", 5000);
        send_dap_request(&mut writer, 10, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    }).await;
    assert!(result.is_ok(), "FB-fields variables test failed: {:?}", result.err());
}

/// Breakpoint resolution at virtual_offset: set a breakpoint inside a
/// helper file in a multi-file project. The attach handler must compute
/// the source's virtual offset (cumulative byte offset of helper.st in
/// the concatenated compilation buffer) before calling
/// `DebugState::set_line_breakpoints`. A regression here turns
/// breakpoints in non-main files into silent no-ops.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_breakpoint_in_helper_file() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();

    let bundle = make_multifile_debug_bundle();
    upload_bundle(&client, &base, &bundle).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client
        .post(format!("{base}/api/v1/program/start"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Find the extracted helper.st on the agent side. The agent writes
    // sources under <program_dir>/current_source/<rel_path>.
    let helper_path = dir.path().join("current_source").join("helper.st");
    assert!(
        helper_path.exists(),
        "agent must have extracted helper.st to {}",
        helper_path.display()
    );
    let helper_path_str = helper_path.to_string_lossy().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);

        // The actual coverage target: setBreakpoints on a non-main file
        // must resolve correctly. Without virtual_offset handling in the
        // attach handler, the line number falls outside the helper's
        // source-map range and the breakpoint comes back unverified
        // (or attached to the wrong line in main.st). Setting it on the
        // body line of HelperFb exercises the resolver path that adds
        // the helper file's virtual offset before line lookup.
        send_dap_request(&mut writer, 3, "setBreakpoints", serde_json::json!({
            "source": { "path": helper_path_str },
            "breakpoints": [{ "line": 8 }]
        }));
        let bp = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "setBreakpoints", 5000);
        assert_eq!(bp["success"], true, "setBreakpoints must succeed: {bp}");
        let bps = bp["body"]["breakpoints"].as_array().expect("breakpoints array");
        assert_eq!(bps.len(), 1, "must echo back 1 breakpoint, got {bp}");
        // helper.st:8 (`total := total + inc;`) MUST be verified — that's
        // the virtual_offset coverage. A miscalculation either rejects
        // it or attaches it to a different file's instruction.
        assert_eq!(
            bps[0]["verified"], true,
            "helper.st:8 must be verified (virtual_offset must shift the lookup): got {bp}"
        );
        assert_eq!(bps[0]["line"], 8, "echoed line must be 8");

        send_dap_request(&mut writer, 4, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

        // Disconnect cleanly. We don't wait for the breakpoint to fire
        // here — the goal is to pin the resolver, not the dispatcher.
        send_dap_request(&mut writer, 5, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    }).await;
    assert!(result.is_ok(), "multi-file breakpoint test failed: {:?}", result.err());
}

// ── runtime_manager::handle_debug_commands paused-state coverage ────────
//
// `handle_debug_commands` (st-target-agent/src/runtime_manager.rs:785-940)
// is the dispatcher for runtime-side debug commands while the VM is
// paused. The wait-and-see review (design_core.md "Test Coverage
// Strategy") identified its branches as the highest-ROI remaining
// cold spot. These tests drive each reachable arm through the real
// TCP DAP proxy + HTTP API — no mocks, no in-process channel surgery.
//
// Coverage targets (see design_core.md for the limits-of-no-mocking
// note on the 30-min timeout and the channel-disconnected branch):
//   * StepIn / StepOver / StepOut       → DAP `stepIn` / `next` / `stepOut`
//   * Evaluate while paused             → DAP `evaluate`
//   * ClearBreakpoints                  → DAP `setBreakpoints` with empty array
//   * RuntimeCommand::ForceVariable     → POST /api/v1/variables/force
//   * RuntimeCommand::UnforceVariable   → DELETE /api/v1/variables/force/:name
//   * RuntimeCommand::Stop              → POST /api/v1/program/stop
//   * RuntimeCommand::DebugAttach swap  → second DAP client connects while first paused
//
// Helper that does the boilerplate: attach over TCP, pause the engine,
// hand back the stream + reader so each test can run command-specific
// assertions and disconnect cleanly.
fn paused_session<F>(dap_port: u16, body: F)
where
    F: FnOnce(&mut TcpStream, &mut BufReader<&TcpStream>, &mut i64) + Send + 'static,
{
    let result = std::thread::spawn(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;

        let mut seq: i64 = 1;
        send_dap_request(&mut writer, seq, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        seq += 1;
        send_dap_request(&mut writer, seq, "attach", serde_json::json!({"stopOnEntry": false}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
        seq += 1;
        send_dap_request(&mut writer, seq, "configurationDone", serde_json::Value::Null);
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);
        seq += 1;

        send_dap_request(&mut writer, seq, "pause", serde_json::json!({"threadId": 1}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "pause", 5000);
        let _ = read_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 5000);
        seq += 1;

        body(&mut writer, &mut reader, &mut seq);

        // Best-effort disconnect — body() may have already disconnected.
        send_dap_request(&mut writer, seq, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    })
    .join();
    assert!(result.is_ok(), "paused-session body panicked: {:?}", result.err());
}

/// `DebugCommand::StepIn`: sending DAP `stepIn` while paused must produce
/// a `Resumed` response and a subsequent `stopped(step)` event from the
/// next instruction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_step_in_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    paused_session(dap_port, |writer, reader, seq| {
        *seq += 1;
        send_dap_request(writer, *seq, "stepIn", serde_json::json!({"threadId": 1}));
        let resp = read_until(reader, |m| m["type"] == "response" && m["command"] == "stepIn", 5000);
        assert_eq!(resp["success"], true, "stepIn must succeed: {resp}");
        // The engine should re-stop on the next source line.
        let stopped = read_until(reader, |m| m["type"] == "event" && m["event"] == "stopped", 5000);
        assert_eq!(stopped["body"]["reason"], "step", "expected step reason: {stopped}");
        *seq += 1;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running", "engine must resume after disconnect");
}

/// `DebugCommand::StepOver`: DAP `next` while paused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_step_over_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    paused_session(dap_port, |writer, reader, seq| {
        *seq += 1;
        send_dap_request(writer, *seq, "next", serde_json::json!({"threadId": 1}));
        let resp = read_until(reader, |m| m["type"] == "response" && m["command"] == "next", 5000);
        assert_eq!(resp["success"], true, "next (stepOver) must succeed: {resp}");
        let stopped = read_until(reader, |m| m["type"] == "event" && m["event"] == "stopped", 5000);
        assert_eq!(stopped["body"]["reason"], "step", "expected step reason: {stopped}");
        *seq += 1;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running");
}

/// `DebugCommand::StepOut`: DAP `stepOut` while paused at the program's
/// top-level body. With no caller frame to return to, the dispatcher's
/// `StepOut` arm still resumes; we verify the response succeeds and the
/// engine ends up running again after disconnect (no deadlock).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_step_out_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    paused_session(dap_port, |writer, reader, seq| {
        *seq += 1;
        send_dap_request(writer, *seq, "stepOut", serde_json::json!({"threadId": 1}));
        let resp = read_until(reader, |m| m["type"] == "response" && m["command"] == "stepOut", 5000);
        assert_eq!(resp["success"], true, "stepOut must succeed: {resp}");
        // Don't require a stopped event — at top-level there's nothing
        // to step out OF; the engine just resumes. The disconnect at the
        // end of the helper unblocks any pending wait.
        *seq += 1;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running");
}

/// `DebugCommand::Evaluate`: DAP `evaluate` against a paused program
/// must return the live value of a local variable.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_evaluate_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    // Let the counter advance well past zero before pausing.
    tokio::time::sleep(Duration::from_millis(800)).await;

    paused_session(dap_port, |writer, reader, seq| {
        *seq += 1;
        send_dap_request(writer, *seq, "evaluate", serde_json::json!({
            "expression": "counter",
            "frameId": 0,
            "context": "watch"
        }));
        let resp = read_until(reader, |m| m["type"] == "response" && m["command"] == "evaluate", 5000);
        assert_eq!(resp["success"], true, "evaluate must succeed: {resp}");
        // The make_debug_bundle program does `counter := counter + 1` each
        // cycle. After 800ms of 10ms cycles, counter should be a positive
        // integer; we just assert it parses and is > 0.
        let result_str = resp["body"]["result"].as_str().unwrap_or("");
        let val: i64 = result_str.parse().unwrap_or_else(|_| panic!(
            "evaluate(counter) must return an integer string, got {result_str:?}"
        ));
        assert!(val > 0, "counter should be > 0, got {val}");
        *seq += 1;
    });
}

/// `DebugCommand::ClearBreakpoints`: a `setBreakpoints` request with an
/// empty `breakpoints` array clears all breakpoints in that source. The
/// dispatcher's `ClearBreakpoints` arm runs when no lines are passed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_clear_breakpoints_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let main_path = dir.path().join("current_source").join("main.st").to_string_lossy().into_owned();

    paused_session(dap_port, move |writer, reader, seq| {
        // Set a breakpoint on the body line.
        *seq += 1;
        send_dap_request(writer, *seq, "setBreakpoints", serde_json::json!({
            "source": { "path": main_path.clone() },
            "breakpoints": [{ "line": 6 }]
        }));
        let set = read_until(reader, |m| m["type"] == "response" && m["command"] == "setBreakpoints", 5000);
        assert_eq!(set["success"], true);
        let bps = set["body"]["breakpoints"].as_array().expect("bps array");
        assert_eq!(bps.len(), 1);

        // Clear it: setBreakpoints with empty array.
        *seq += 1;
        send_dap_request(writer, *seq, "setBreakpoints", serde_json::json!({
            "source": { "path": main_path.clone() },
            "breakpoints": []
        }));
        let cleared = read_until(reader, |m| m["type"] == "response" && m["command"] == "setBreakpoints", 5000);
        assert_eq!(cleared["success"], true);
        let bps2 = cleared["body"]["breakpoints"].as_array().expect("bps array");
        assert!(bps2.is_empty(), "cleared breakpoints array must be empty: {cleared}");

        // Resume and verify — engine must keep running, not re-trip a stale BP.
        *seq += 1;
        send_dap_request(writer, *seq, "continue", serde_json::json!({"threadId": 1}));
        let _ = read_until(reader, |m| m["type"] == "response" && m["command"] == "continue", 5000);
        *seq += 1;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let status: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["status"], "running", "engine must keep running after BP cleared");
}

/// Force around a debug session: a forced variable set BEFORE pause
/// must remain forced through pause + resume + disconnect. This
/// exercises the cycle-loop's force-application path under the
/// engine-state transitions Running → DebugPaused → Running.
///
/// The original "force WHILE paused" form deadlocks against
/// `handle_debug_commands`'s `recv_timeout` (see design_core.md
/// "RuntimeCommand::ForceVariable while paused — known runtime
/// limitation"). Once the dispatcher migrates to `select!`, restore
/// the in-pause variant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_force_around_debug_session() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 1. Force BEFORE attaching — engine is plain-running, no debug session.
    let r = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "Main.counter", "value": "9999" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "force before pause must succeed");

    // Verify it took effect.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let pre_pause: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send().await.unwrap().json().await.unwrap();
    let v = pre_pause["variables"].as_array().expect("array")
        .iter().find(|v| v["name"].as_str().map(|s| s.eq_ignore_ascii_case("Main.counter")).unwrap_or(false))
        .expect("Main.counter present");
    assert_eq!(v["value"].as_str(), Some("9999"), "force must take effect before pause");
    assert_eq!(v["forced"], true);

    // 2. Attach + pause + (read variables to confirm force survives) +
    //    continue + disconnect.
    paused_session(dap_port, move |writer, reader, seq| {
        // Read locals while paused — the forced value must persist.
        *seq += 1;
        send_dap_request(writer, *seq, "stackTrace", serde_json::json!({"threadId": 1, "startFrame": 0, "levels": 1}));
        let st = read_until(reader, |m| m["type"] == "response" && m["command"] == "stackTrace", 5000);
        let frame_id = st["body"]["stackFrames"][0]["id"].as_i64().unwrap();

        *seq += 1;
        send_dap_request(writer, *seq, "scopes", serde_json::json!({"frameId": frame_id}));
        let scopes = read_until(reader, |m| m["type"] == "response" && m["command"] == "scopes", 5000);
        let locals_ref = scopes["body"]["scopes"].as_array().unwrap()
            .iter().find(|s| s["name"] == "Locals").unwrap()["variablesReference"].as_i64().unwrap();

        *seq += 1;
        send_dap_request(writer, *seq, "variables", serde_json::json!({"variablesReference": locals_ref}));
        let vars = read_until(reader, |m| m["type"] == "response" && m["command"] == "variables", 5000);
        let counter = vars["body"]["variables"].as_array().unwrap()
            .iter().find(|v| v["name"].as_str().map(|s| s.eq_ignore_ascii_case("counter")).unwrap_or(false))
            .unwrap_or_else(|| panic!("counter not found in {vars}"));
        assert_eq!(
            counter["value"].as_str(), Some("9999"),
            "forced counter must remain 9999 across pause: {counter}"
        );

        *seq += 1;
        send_dap_request(writer, *seq, "continue", serde_json::json!({"threadId": 1}));
        let _ = read_until(reader, |m| m["type"] == "response" && m["command"] == "continue", 5000);
        *seq += 1;
    });

    // 3. After disconnect, the force must STILL be in effect — the
    //    dispatcher's pause/resume cycle didn't drop it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let post_session: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send().await.unwrap().json().await.unwrap();
    let v = post_session["variables"].as_array().expect("array")
        .iter().find(|v| v["name"].as_str().map(|s| s.eq_ignore_ascii_case("Main.counter")).unwrap_or(false))
        .expect("Main.counter present");
    assert_eq!(v["value"].as_str(), Some("9999"), "force must persist after disconnect");
    assert_eq!(v["forced"], true);

    // 4. Unforce, verify it sticks across a final pause/resume too — this
    //    exercises the runtime's force-state mutability after a debug
    //    detach has cleaned up.
    let r = client
        .delete(format!("{base}/api/v1/variables/force/Main.counter"))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let unforced: serde_json::Value = client
        .get(format!("{base}/api/v1/variables?watch=Main.counter"))
        .send().await.unwrap().json().await.unwrap();
    let v = unforced["variables"].as_array().expect("array")
        .iter().find(|v| v["name"].as_str().map(|s| s.eq_ignore_ascii_case("Main.counter")).unwrap_or(false))
        .expect("Main.counter present");
    assert_eq!(v["forced"], false, "unforce must take effect post-debug");

    let _ = client.post(format!("{base}/api/v1/program/stop")).send().await;
}

/// `RuntimeCommand::Stop` while paused: the runtime thread accepts Stop
/// even when the VM is halted in `handle_debug_commands`, returns
/// `DebugAction::Stop`, exits the cycle loop cleanly, and the agent
/// reports idle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_http_stop_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let base_for_thread = base.clone();
    paused_session(dap_port, move |_writer, _reader, _seq| {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let stop_status = rt.block_on(async {
            let c = reqwest::Client::new();
            c.post(format!("{base_for_thread}/api/v1/program/stop"))
                .send().await.unwrap()
                .status().as_u16()
        });
        assert_eq!(stop_status, 200, "Stop while paused must succeed");
    });

    // Engine must end up idle — the dispatcher's Stop arm exits
    // handle_debug_commands and then the cycle loop.
    let mut idle = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let s: serde_json::Value = client.get(format!("{base}/api/v1/status")).send().await.unwrap().json().await.unwrap();
        if s["status"].as_str() == Some("idle") { idle = true; break; }
    }
    assert!(idle, "agent must reach idle after Stop-while-paused");
}

/// `RuntimeCommand::DebugAttach` while a session is paused: a second
/// DAP client connecting takes over, and the first session receives a
/// `Detached` event. This is the swap branch in handle_debug_commands.
///
/// NOTE: The current dap_proxy implementation rejects a second TCP
/// connection at the proxy layer (single-session lock), before the
/// runtime-level swap branch can run. We document that here and pin
/// the proxy-layer behaviour we DO get; the runtime-side swap branch
/// remains deferred until either the proxy gains multi-client support
/// or the swap happens via an in-process API.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dap_attach_second_connection_rejected_while_first_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, dap_port, _h) = start_agent_with_dap(test_config(dir.path())).await;
    let client = Client::new();
    upload_bundle(&client, &base, &make_debug_bundle()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.post(format!("{base}/api/v1/program/start")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Hold session 1 in paused state, then probe with a second connection.
    paused_session(dap_port, move |_writer, _reader, _seq| {
        // Spawn a second TCP connection and try to initialize+attach.
        // The proxy may close the connection abruptly (RST), so the
        // probe wraps reads in catch_unwind — ConnectionReset → "rejected".
        let probe = std::thread::spawn(move || -> bool {
            let stream = match TcpStream::connect(format!("127.0.0.1:{dap_port}")) {
                Ok(s) => s,
                Err(_) => return false, // connect refused → rejected
            };
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let reader_stream = stream.try_clone().unwrap();
            let mut reader = BufReader::new(&reader_stream);
            let mut writer = stream;

            send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
            // catch_unwind absorbs any read panic (ConnectionReset, etc.)
            // and treats it as "the proxy closed on us" → rejected.
            let init = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                read_dap_message_timeout(&mut reader)
            }));
            let init = match init {
                Ok(opt) => opt,
                Err(_) => return false, // panic on read → rejected
            };
            match init {
                None => false, // closed cleanly → rejected
                Some(m) => {
                    if m["success"].as_bool() == Some(false) {
                        false // explicit rejection
                    } else {
                        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
                        let attach = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            read_dap_message_timeout(&mut reader)
                        }))
                        .ok()
                        .flatten();
                        attach.map(|m| m["success"].as_bool() == Some(true)).unwrap_or(false)
                    }
                }
            }
        });

        let second_connected_ok = probe.join().unwrap_or(true);
        assert!(
            !second_connected_ok,
            "Second DAP client must be rejected while the first holds an active session"
        );
    });

    // After session 1 disconnects, a fresh session must succeed —
    // proves the lock is released, not stuck.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let reattach = std::thread::spawn(move || {
        let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}")).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        let mut writer = stream;
        send_dap_request(&mut writer, 1, "initialize", serde_json::json!({"adapterID":"st"}));
        let _ = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 5000);
        send_dap_request(&mut writer, 2, "attach", serde_json::json!({"stopOnEntry": false}));
        let r = read_until(&mut reader, |m| m["type"] == "response" && m["command"] == "attach", 5000);
        assert_eq!(r["success"], true, "fresh attach after lock release must succeed");
        send_dap_request(&mut writer, 3, "disconnect", serde_json::json!({"terminateDebuggee": false}));
    }).join();
    assert!(reattach.is_ok());
}
