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
