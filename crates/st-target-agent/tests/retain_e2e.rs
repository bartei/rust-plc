//! End-to-end tests for IEC 61131-3 RETAIN / PERSISTENT variable
//! persistence across the full agent stack:
//!
//!   compile bundle → upload via HTTP → start engine → force values → stop
//!   → on-disk `.retain` snapshot exists → start again → values are restored
//!
//! These tests run in-process — they boot the same axum router that the
//! systemd service runs and hit it with a real `reqwest` client. They are
//! the local-CI equivalent of the QEMU `e2e_x86_64_retain_across_restart`
//! suite (which exercises the same flow against a real Debian guest with
//! systemd controlling the agent process).
//!
//! The retain directory is overridden via
//! `RuntimeManager::new_with_retain_dir` so the test runs unprivileged
//! against a tmpfs path instead of `/var/lib/st-plc/retain`.

use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::Value;
use st_deploy::bundle::{create_bundle, write_bundle, BundleOptions};
use st_target_agent::config::AgentConfig;
use st_target_agent::server::{build_app_state, build_router};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite;

// ── Test harness ──────────────────────────────────────────────────────

/// Start a test agent on a random port. `retain_dir` overrides the agent's
/// retain directory so we can write to a tempdir instead of /var/lib/st-plc.
async fn start_agent(
    program_dir: &Path,
    retain_dir: &Path,
) -> (String, tokio::task::JoinHandle<()>) {
    let mut config = AgentConfig::default();
    config.storage.program_dir = program_dir.to_path_buf();
    config.storage.retain_dir = retain_dir.to_path_buf();

    let state = build_app_state(config, None).unwrap();
    let router = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

/// Build the test-project-retain bundle from the on-disk fixture.
fn retain_bundle() -> Vec<u8> {
    bundle_for_fixture("test-project-retain")
}

fn bundle_for_fixture(name: &str) -> Vec<u8> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/e2e-deploy/fixtures")
        .join(name);
    let bundle = create_bundle(&fixture, &BundleOptions::default())
        .unwrap_or_else(|e| panic!("failed to create bundle for {name}: {e}"));
    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_bundle(&bundle, tmp.path()).unwrap();
    std::fs::read(tmp.path()).unwrap()
}

async fn upload_bundle(client: &Client, base: &str, data: &[u8]) -> reqwest::Response {
    let part = reqwest::multipart::Part::bytes(data.to_vec())
        .file_name("retain.st-bundle");
    let form = reqwest::multipart::Form::new().part("file", part);
    client
        .post(format!("{base}/api/v1/program/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap()
}

async fn get_json(client: &Client, url: String) -> (u16, Value) {
    let resp = client.get(&url).send().await.unwrap();
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn post_empty(client: &Client, url: String) -> u16 {
    client.post(&url).send().await.unwrap().status().as_u16()
}

/// Poll `/api/v1/status` until the cycle counter exceeds `min_cycles`,
/// then return the final value of `var_name` from the variables endpoint.
async fn wait_for_cycles_and_read(
    client: &Client,
    base: &str,
    min_cycles: u64,
    var_name: &str,
) -> String {
    for _ in 0..50 {
        let (_, status) = get_json(client, format!("{base}/api/v1/status")).await;
        let cycles = status["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
        if cycles >= min_cycles {
            let (_, vars) = get_json(
                client,
                format!("{base}/api/v1/variables?watch={var_name}"),
            )
            .await;
            for v in vars["variables"].as_array().unwrap_or(&vec![]) {
                if v["name"].as_str().unwrap_or("").eq_ignore_ascii_case(var_name) {
                    return v["value"].as_str().unwrap_or("").to_string();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {var_name} to advance past {min_cycles} cycles");
}

// ── Tests ────────────────────────────────────────────────────────────

/// Catalog must surface RETAIN / PERSISTENT bits so the monitor UI can
/// render badges. This is the cheap structural check — no engine restart.
#[tokio::test]
async fn catalog_carries_retain_persistent_flags() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    let bundle = retain_bundle();
    let resp = upload_bundle(&client, &base, &bundle).await;
    assert_eq!(resp.status(), 200);

    assert_eq!(post_empty(&client, format!("{base}/api/v1/program/start")).await, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (_, catalog) = get_json(&client, format!("{base}/api/v1/variables/catalog")).await;
    let entries = catalog["variables"]
        .as_array()
        .expect("catalog must have entries");

    let find = |name: &str| -> Value {
        entries
            .iter()
            .find(|e| e["name"].as_str().unwrap_or("").eq_ignore_ascii_case(name))
            .cloned()
            .unwrap_or(Value::Null)
    };

    let r = find("g_retain_counter");
    assert_eq!(r["retain"], true, "g_retain_counter must be RETAIN: {r}");
    assert!(
        r["persistent"].as_bool() != Some(true),
        "g_retain_counter is RETAIN-only, not PERSISTENT"
    );

    let p = find("g_persistent_total");
    assert!(
        p["persistent"].as_bool() == Some(true),
        "g_persistent_total must be PERSISTENT: {p}"
    );

    let rp = find("g_durable");
    assert_eq!(rp["retain"], true, "g_durable must be RETAIN");
    assert_eq!(rp["persistent"], true, "g_durable must be PERSISTENT");

    // Negative control: cycle_count is a plain local — neither flag set.
    let plain = find("Main.cycle_count");
    assert!(
        plain["retain"].as_bool() != Some(true)
            && plain["persistent"].as_bool() != Some(true),
        "Main.cycle_count is plain, must not carry retain/persistent: {plain}"
    );

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

/// The defining E2E test: forced RETAIN values survive the program being
/// stopped and started again. This is the contract IEC 61131-3 RETAIN
/// semantics promise — it exercises the full save-on-stop / restore-on-
/// start path through the runtime manager and the on-disk JSON snapshot.
#[tokio::test]
async fn retain_values_survive_stop_restart() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    // Phase 1: deploy + start. Let the engine run a few cycles.
    let bundle = retain_bundle();
    assert_eq!(upload_bundle(&client, &base, &bundle).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );

    // Wait for cycles to advance, then force three values:
    // - g_retain_counter (RETAIN)        → 4242
    // - g_persistent_total (PERSISTENT)  → 999999
    // - g_durable (RETAIN PERSISTENT)    → 7777
    let _ = wait_for_cycles_and_read(&client, &base, 5, "g_retain_counter").await;

    for (name, value) in [
        ("g_retain_counter", "4242"),
        ("g_persistent_total", "999999"),
        ("g_durable", "7777"),
    ] {
        let body = serde_json::json!({ "name": name, "value": value });
        let r = client
            .post(format!("{base}/api/v1/variables/force"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "force {name} -> {value} failed");
    }

    // Let the cycle apply the forces and run a checkpoint or two.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Unforce so the saved value isn't continually re-stamped — IEC RETAIN
    // is about persisting the LAST scalar value, not the force.
    for name in ["g_retain_counter", "g_persistent_total", "g_durable"] {
        let r = client
            .delete(format!("{base}/api/v1/variables/force/{name}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "unforce {name} failed");
    }

    // Phase 2: stop the program. The runtime thread should call
    // engine.save_retain() before dropping the engine.
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/stop")).await,
        200
    );
    // Stop is async via the command channel — wait for status to flip.
    for _ in 0..40 {
        let (_, s) = get_json(&client, format!("{base}/api/v1/status")).await;
        if s["status"].as_str() == Some("idle") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // The retain file must be on disk. Filename is `<program>.retain`.
    let retain_file = retain_dir.path().join("Main.retain");
    assert!(
        retain_file.exists(),
        "retain file missing at {}",
        retain_file.display()
    );

    let content = std::fs::read_to_string(&retain_file).expect("read retain file");
    assert!(
        content.contains("g_retain_counter"),
        "retain file must mention g_retain_counter:\n{content}"
    );
    assert!(
        content.contains("g_persistent_total"),
        "retain file must mention g_persistent_total"
    );
    assert!(
        content.contains("g_durable"),
        "retain file must mention g_durable"
    );

    // Phase 3: start the program again. The engine constructor restores
    // RETAIN values (warm restart). After restart, all three retained
    // globals should still hold the values we last forced — even though
    // initialisers say `:= 0` and the program increments them every cycle.
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );

    // Read the values immediately (before too many increments).
    //
    // IEC 61131-3 / this engine's warm-restart semantics
    // (retain_store::restore_snapshot called with warm=true on Engine::new):
    //   - RETAIN              → restored
    //   - PERSISTENT only     → NOT restored (resets to initialiser)
    //   - RETAIN PERSISTENT   → restored (RETAIN bit covers warm restart)
    //
    // The program adds +1 each cycle, so the values will quickly grow past
    // the forced numbers — we just assert they didn't drop back to zero.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let counter_after = wait_for_cycles_and_read(&client, &base, 1, "g_retain_counter").await;
    let counter_n: i64 = counter_after.parse().expect("counter is integer");
    assert!(
        counter_n >= 4242,
        "RETAIN: g_retain_counter dropped to {counter_n} after warm restart (expected >= 4242)"
    );

    let durable_after = wait_for_cycles_and_read(&client, &base, 1, "g_durable").await;
    let durable_n: i64 = durable_after.parse().expect("durable is integer");
    assert!(
        durable_n >= 7777,
        "RETAIN PERSISTENT: g_durable dropped to {durable_n} after warm restart (expected >= 7777)"
    );

    // PERSISTENT-only is NOT restored on warm restart — this is the IEC
    // contract. Verify it actually reset (didn't accidentally survive).
    let total_after = wait_for_cycles_and_read(&client, &base, 1, "g_persistent_total").await;
    let total_n: i64 = total_after.parse().expect("total is integer");
    assert!(
        total_n < 1_000,
        "PERSISTENT-only must reset on warm restart, got {total_n}"
    );

    // The on-disk snapshot still records the persistent value (so a cold
    // restart from this snapshot would restore it). We already verified
    // above that the file mentions g_persistent_total.

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

/// The WebSocket monitor protocol is what the VS Code Monitor panel
/// actually uses. This test exercises that end-to-end: catalog responses
/// must carry retain/persistent flags, and pushed `variableUpdate`
/// `watch_tree` nodes must carry the same flags so the panel can render
/// the badge.
#[tokio::test]
async fn ws_catalog_and_watch_tree_carry_retain_flags() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    let bundle = retain_bundle();
    assert_eq!(upload_bundle(&client, &base, &bundle).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ── 1. WS getCatalog ─────────────────────────────────────────
    let ws_url = base.replace("http://", "ws://") + "/api/v1/monitor/ws";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&serde_json::json!({ "method": "getCatalog" }))
            .unwrap(),
    ))
    .await
    .unwrap();

    // Skip variableUpdate pushes until we get the catalog response.
    let catalog = loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("ws timeout")
            .expect("ws closed")
            .expect("ws error");
        if let tungstenite::Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["type"] == "catalog" {
                break v;
            }
        }
    };

    let vars = catalog["variables"].as_array().expect("vars array");
    let find_ws = |name: &str| -> Value {
        vars.iter()
            .find(|e| e["name"].as_str().unwrap_or("").eq_ignore_ascii_case(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(
        find_ws("g_retain_counter")["retain"], true,
        "WS catalog must carry retain bit"
    );
    assert_eq!(
        find_ws("g_durable")["persistent"], true,
        "WS catalog must carry persistent bit"
    );

    // ── 2. Subscribe and verify pushed watch_tree carries flags ───
    ws.send(tungstenite::Message::Text(
        serde_json::to_string(&serde_json::json!({
            "method": "subscribe",
            "params": { "variables": ["g_retain_counter", "g_durable", "g_persistent_total"] }
        }))
        .unwrap(),
    ))
    .await
    .unwrap();

    // Wait for a pushed variableUpdate.
    let push = loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("ws push timeout")
            .expect("ws closed")
            .expect("ws error");
        if let tungstenite::Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["type"] == "variableUpdate" {
                break v;
            }
        }
    };

    let tree = push["watch_tree"].as_array().expect("watch_tree array");
    let find_node = |name: &str| -> Value {
        tree.iter()
            .find(|n| n["fullPath"].as_str().unwrap_or("").eq_ignore_ascii_case(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(
        find_node("g_retain_counter")["retain"], true,
        "watch_tree node must carry retain"
    );
    assert_eq!(
        find_node("g_durable")["retain"], true,
        "watch_tree g_durable retain"
    );
    assert_eq!(
        find_node("g_durable")["persistent"], true,
        "watch_tree g_durable persistent"
    );
    let plain = find_node("g_persistent_total");
    // PERSISTENT-only must show persistent=true and retain=false.
    assert_eq!(plain["persistent"], true);
    assert!(plain["retain"].as_bool() != Some(true));

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

/// PROGRAM locals declared without a retain qualifier must NOT survive
/// a stop/restart — they're part of the warm restart "reset" group.
#[tokio::test]
async fn non_retain_locals_reset_on_restart() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    let bundle = retain_bundle();
    assert_eq!(upload_bundle(&client, &base, &bundle).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );

    // Let cycle_count climb well past zero.
    let _ = wait_for_cycles_and_read(&client, &base, 20, "Main.cycle_count").await;

    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/stop")).await,
        200
    );
    for _ in 0..40 {
        let (_, s) = get_json(&client, format!("{base}/api/v1/status")).await;
        if s["status"].as_str() == Some("idle") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );
    // Read straight after startup — the very first cycle just bumped from
    // 0 to 1 (no retain), so the value should be small. We allow up to 50
    // to account for cycle-loop startup race.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let after = wait_for_cycles_and_read(&client, &base, 1, "Main.cycle_count").await;
    let n: i64 = after.parse().expect("cycle_count is integer");
    assert!(
        n < 50,
        "non-retain Main.cycle_count must reset to ~0 after restart, got {n}"
    );

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

// ── retain_store deeper coverage ───────────────────────────────────────
//
// The plan calls out four sub-areas in `retain_store.rs` that need
// coverage:
//   * `capture_instance_fields` — exercised here via a RETAIN FB instance
//   * `restore_snapshot` is_compatible check — exercised by redeploying
//     a program where a retained var changed type
//   * `load_from_file` error branch — exercised by pre-writing a corrupt
//     retain file before agent boot
//   * warm vs. cold restore — already covered by retain_values_survive_
//     stop_restart for warm. The `warm=false` (cold) branch is **deferred**:
//     no system-level entry point in the agent invokes it. See the
//     deferred note in plan/implementation.md for details.
//   * `save_to_file` filesystem error branches — also **deferred**:
//     the only realistic ways to fail std::fs::write/rename are read-only
//     filesystems or out-of-space conditions, both of which are too OS-
//     fragile for CI and provide little actual safety value.

/// `capture_instance_fields` for FB instances: a `VAR RETAIN` FB inside
/// the program must have its scalar fields captured into the retain
/// snapshot and restored on warm restart.
#[tokio::test]
async fn fb_instance_retain_fields_survive_restart() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    let bundle = bundle_for_fixture("test-project-retain-fb");
    assert_eq!(upload_bundle(&client, &base, &bundle).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );

    // Force the FB's CV field to a known value via the dotted-path
    // monitor API. This exercises the same fb_instances write path that
    // capture_instance_fields will later read back from.
    let _ = wait_for_cycles_and_read(&client, &base, 5, "Main.fb.cv").await;
    let body = serde_json::json!({ "name": "Main.fb.cv", "value": "1234" });
    let r = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Unforce so the saved value reflects the program's last live write
    // rather than a perpetual override. We expect the retain machinery
    // to capture whatever was in fb_instances at stop time.
    let r = client
        .delete(format!("{base}/api/v1/variables/force/Main.fb.cv"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // Stop — runtime_thread calls engine.save_retain() before dropping.
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/stop")).await,
        200
    );
    for _ in 0..40 {
        let (_, s) = get_json(&client, format!("{base}/api/v1/status")).await;
        if s["status"].as_str() == Some("idle") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Snapshot file must contain the FB instance under instance_fields.
    let snap = retain_dir.path().join("Main.retain");
    let content = std::fs::read_to_string(&snap).expect("retain file");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("retain JSON");
    let fb_fields = parsed["instance_fields"]["Main"]["fb"]
        .as_object()
        .unwrap_or_else(|| panic!("instance_fields.Main.fb missing in {content}"));
    assert!(
        fb_fields.contains_key("cv"),
        "FB.cv must be captured, got fields {:?}",
        fb_fields.keys().collect::<Vec<_>>()
    );

    // Restart — engine reads the snapshot and reseats fb_instances.
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );
    tokio::time::sleep(Duration::from_millis(80)).await;

    let after = wait_for_cycles_and_read(&client, &base, 1, "Main.fb.cv").await;
    let cv: i64 = after.parse().expect("cv is integer");
    assert!(
        cv >= 1234,
        "RETAIN FB field cv must survive warm restart (got {cv}, expected >= 1234)"
    );

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

/// `is_compatible` corner case: the snapshot on disk says g_retain_counter
/// is INT, but the freshly-deployed program declares it as REAL. Restore
/// must skip the incompatible entry (logging a "type mismatch" warning)
/// and the new program must boot cleanly with the default REAL value
/// rather than crashing or retaining the wrong type.
#[tokio::test]
async fn retain_restore_skips_incompatible_types() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();
    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    // Phase 1: deploy v1 (g_retain_counter : INT), force a value, stop.
    let v1 = retain_bundle();
    assert_eq!(upload_bundle(&client, &base, &v1).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );
    let _ = wait_for_cycles_and_read(&client, &base, 5, "g_retain_counter").await;

    let r = client
        .post(format!("{base}/api/v1/variables/force"))
        .json(&serde_json::json!({ "name": "g_retain_counter", "value": "9999" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = client
        .delete(format!("{base}/api/v1/variables/force/g_retain_counter"))
        .send()
        .await
        .unwrap();

    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/stop")).await,
        200
    );
    for _ in 0..40 {
        let (_, s) = get_json(&client, format!("{base}/api/v1/status")).await;
        if s["status"].as_str() == Some("idle") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let snap = retain_dir.path().join("Main.retain");
    assert!(snap.exists(), "v1 retain snapshot must exist");
    let v1_content = std::fs::read_to_string(&snap).unwrap();
    assert!(
        v1_content.contains("\"Int\""),
        "v1 snapshot must store Int values: {v1_content}"
    );

    // Phase 2: deploy v2 where g_retain_counter is REAL — same name,
    // different type. The agent calls store_bundle which replaces the
    // program; engine startup will then try to restore the incompatible
    // INT entry and must silently skip it.
    let v2 = bundle_for_fixture("test-project-retain-typechange");
    assert_eq!(upload_bundle(&client, &base, &v2).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200
    );
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Sanity: program is running. is_compatible rejected the INT→REAL
    // restore; the variable holds the default REAL value (0.0), then
    // increments by 1.0 each cycle. After ~10ms it should be < 100.
    let (status_code, status) = get_json(&client, format!("{base}/api/v1/status")).await;
    assert_eq!(status_code, 200);
    assert_eq!(
        status["status"], "running",
        "v2 must run after incompatible restore, got status={status}"
    );

    // Read g_retain_counter — it should be a small REAL value, NOT 9999.
    let (_, vars) = get_json(
        &client,
        format!("{base}/api/v1/variables?watch=g_retain_counter"),
    )
    .await;
    let arr = vars["variables"].as_array().expect("variables");
    let v = arr
        .iter()
        .find(|v| v["name"].as_str().unwrap_or("") == "g_retain_counter")
        .expect("g_retain_counter in variables");
    let val_str = v["value"].as_str().unwrap_or("");
    let val: f64 = val_str.parse().unwrap_or(f64::INFINITY);
    assert!(
        val < 9000.0,
        "incompatible INT restore must NOT carry over the 9999 value into the REAL slot, got {val_str}"
    );
    // And the type field reflects the new declaration.
    let ty = v["type"].as_str().unwrap_or("");
    assert!(
        ty.eq_ignore_ascii_case("REAL"),
        "v2 declares g_retain_counter as REAL, type field should reflect that, got {ty}"
    );

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}

/// `load_from_file` error branch: a corrupt JSON file in the retain dir
/// must not prevent the program from starting. The engine logs a warning
/// and proceeds with default values.
#[tokio::test]
async fn corrupt_retain_file_is_logged_not_fatal() {
    let prog_dir = tempfile::tempdir().unwrap();
    let retain_dir = tempfile::tempdir().unwrap();

    // Pre-write garbage at the path the agent will look at on engine
    // start — `<retain_dir>/<program_name>.retain`.
    std::fs::write(
        retain_dir.path().join("Main.retain"),
        "{ not valid json at all }}}",
    )
    .unwrap();

    let (base, _h) = start_agent(prog_dir.path(), retain_dir.path()).await;
    let client = Client::new();

    let bundle = retain_bundle();
    assert_eq!(upload_bundle(&client, &base, &bundle).await.status(), 200);
    assert_eq!(
        post_empty(&client, format!("{base}/api/v1/program/start")).await,
        200,
    );

    // The program must reach Running despite the unreadable retain file.
    let mut running = false;
    for _ in 0..40 {
        let (_, s) = get_json(&client, format!("{base}/api/v1/status")).await;
        if s["status"].as_str() == Some("running") {
            running = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(running, "engine must start even when the retain file is corrupt");

    // RETAIN globals must boot to their declared initialisers (g_retain_counter
    // initialises to 0 and bumps once per cycle), NOT carry whatever the
    // corrupt file claimed. Read after a small delay; the value must be
    // small (no >1M garbage from a deserialization-gone-wrong).
    tokio::time::sleep(Duration::from_millis(80)).await;
    let counter = wait_for_cycles_and_read(&client, &base, 1, "g_retain_counter").await;
    let n: i64 = counter.parse().expect("counter is integer");
    assert!(
        (0..10_000).contains(&n),
        "g_retain_counter must boot cleanly from initialiser (got {n})"
    );

    let _ = post_empty(&client, format!("{base}/api/v1/program/stop")).await;
}
