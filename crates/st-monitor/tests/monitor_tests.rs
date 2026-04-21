//! Monitor server integration tests.
//!
//! Tests the WebSocket monitor protocol end-to-end: starts a real server,
//! connects a real WebSocket client, sends requests, and verifies responses
//! and pushed variable updates.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use st_monitor::protocol::*;
use st_monitor::server::*;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ── Test helpers ────────────────────────────────────────────────────────

/// Start a monitor server on a random port and return the handle + address.
async fn start_server() -> (MonitorHandle, String) {
    let handle = MonitorHandle::new();
    let addr = run_monitor_server("127.0.0.1:0", handle.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await; // let server start
    (handle, addr.to_string())
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<TcpStream>,
>;

async fn connect(addr: &str) -> WsStream {
    let url = format!("ws://{addr}");
    let (ws, _) = connect_async(&url).await.expect("Failed to connect");
    ws
}

/// Send a JSON request and wait for the next non-push response.
/// Skips any variableUpdate pushes that arrive in between.
async fn request(ws: &mut WsStream, req: Value) -> Value {
    ws.send(Message::Text(serde_json::to_string(&req).unwrap()))
        .await
        .unwrap();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("closed")
            .expect("error");
        if let Message::Text(text) = msg {
            let val: Value = serde_json::from_str(&text).unwrap();
            // Skip pushed variableUpdate messages
            if val.get("type").and_then(|t| t.as_str()) == Some("variableUpdate") {
                continue;
            }
            return val;
        }
    }
}

/// Receive the next variableUpdate that has a non-empty variables array.
/// Skips empty pushes from subscribe-triggered broadcasts.
async fn recv_nonempty(ws: &mut WsStream) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout waiting for non-empty variableUpdate")
            .expect("closed")
            .expect("error");
        if let Message::Text(text) = msg {
            let val: Value = serde_json::from_str(&text).unwrap();
            if val.get("type").and_then(|t| t.as_str()) == Some("variableUpdate") {
                let vars = val.get("variables").and_then(|v| v.as_array());
                if vars.is_some_and(|v| !v.is_empty()) {
                    return val;
                }
                continue; // skip empty pushes
            }
            return val;
        }
    }
}

/// Receive the next message (any type) with timeout.
async fn recv(ws: &mut WsStream) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("closed")
            .expect("error");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

// ── Protocol request/response tests ─────────────────────────────────────

#[tokio::test]
async fn test_subscribe() {
    let (_handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    let resp = request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x", "y"], "interval_ms": 0 }}),
    ).await;

    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["subscribed"], 2);
}

#[tokio::test]
async fn test_unsubscribe() {
    let (_handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x", "y"], "interval_ms": 0 }}),
    ).await;

    let resp = request(
        &mut ws,
        json!({ "method": "unsubscribe", "params": { "variables": ["x"] }}),
    ).await;

    assert_eq!(resp["success"], true);
}

#[tokio::test]
async fn test_read_variables() {
    let (handle, addr) = start_server().await;

    // Populate state
    handle.update_variables(
        vec![
            VariableValue { name: "counter".into(), value: "42".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "running".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, last_cycle_us: 10, min_cycle_us: 10, max_cycle_us: 10, avg_cycle_us: 10, ..Default::default() },
    );

    let mut ws = connect(&addr).await;
    let resp = request(
        &mut ws,
        json!({ "method": "read", "params": { "variables": ["counter", "running"] }}),
    ).await;

    assert_eq!(resp["success"], true);
    let data = resp["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert!(data.iter().any(|v| v["name"] == "counter" && v["value"] == "42"));
}

#[tokio::test]
async fn test_read_nonexistent() {
    let (_handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    let resp = request(
        &mut ws,
        json!({ "method": "read", "params": { "variables": ["nonexistent"] }}),
    ).await;

    assert_eq!(resp["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_catalog() {
    let (handle, addr) = start_server().await;

    handle.set_catalog(vec![
        CatalogEntry { name: "Main.counter".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.flag".into(), var_type: "BOOL".into() },
    ]);

    let mut ws = connect(&addr).await;
    let resp = request(&mut ws, json!({ "method": "getCatalog" })).await;

    assert_eq!(resp["type"], "catalog");
    let vars = resp["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 2);
    assert!(vars.iter().any(|v| v["name"] == "Main.counter" && v["type"] == "INT"));
}

#[tokio::test]
async fn test_get_cycle_info() {
    let (handle, addr) = start_server().await;

    handle.update_variables(
        vec![],
        CycleInfoData { cycle_count: 1000, last_cycle_us: 50, min_cycle_us: 30, max_cycle_us: 120, avg_cycle_us: 55, ..Default::default() },
    );

    let mut ws = connect(&addr).await;
    let resp = request(&mut ws, json!({ "method": "getCycleInfo" })).await;

    assert_eq!(resp["type"], "cycleInfo");
    assert_eq!(resp["cycle_count"], 1000);
    assert_eq!(resp["last_cycle_us"], 50);
}

#[tokio::test]
async fn test_force_variable() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    let resp = request(
        &mut ws,
        json!({ "method": "force", "params": { "variable": "output", "value": 100 }}),
    ).await;

    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["forced"], true);

    // Verify in state
    let forced = handle.peek_forced_variables();
    assert_eq!(forced.get("output"), Some(&st_ir::Value::Int(100)));
}

#[tokio::test]
async fn test_force_bool() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "force", "params": { "variable": "alarm", "value": true }}),
    ).await;

    let forced = handle.peek_forced_variables();
    assert_eq!(forced.get("alarm"), Some(&st_ir::Value::Bool(true)));
}

#[tokio::test]
async fn test_unforce_variable() {
    let (handle, addr) = start_server().await;

    // Pre-force via state
    handle.state().write().unwrap().forced_variables.insert("x".into(), st_ir::Value::Int(0));

    let mut ws = connect(&addr).await;
    let resp = request(
        &mut ws,
        json!({ "method": "unforce", "params": { "variable": "x" }}),
    ).await;

    assert_eq!(resp["success"], true);
    assert!(handle.peek_forced_variables().is_empty());
}

#[tokio::test]
async fn test_invalid_json() {
    let (_handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    ws.send(Message::Text("not json".into())).await.unwrap();

    let resp = recv(&mut ws).await;
    assert_eq!(resp["type"], "error");
    assert!(resp["message"].as_str().unwrap().contains("Invalid"));
}

#[tokio::test]
async fn test_multiple_clients() {
    let (handle, addr) = start_server().await;

    handle.update_variables(
        vec![VariableValue { name: "shared".into(), value: "99".into(), var_type: "INT".into(), forced: false }],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let mut ws1 = connect(&addr).await;
    let mut ws2 = connect(&addr).await;

    let resp1 = request(&mut ws1, json!({ "method": "read", "params": { "variables": ["shared"] }})).await;
    let resp2 = request(&mut ws2, json!({ "method": "read", "params": { "variables": ["shared"] }})).await;

    assert_eq!(resp1["data"][0]["value"], "99");
    assert_eq!(resp2["data"][0]["value"], "99");
}

#[tokio::test]
async fn test_online_change_request() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    let resp = request(
        &mut ws,
        json!({ "method": "onlineChange", "params": { "source": "PROGRAM Main\nEND_PROGRAM" }}),
    ).await;

    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["pending"], true);
    assert!(handle.take_pending_online_change().is_some());
}

// ── Push mechanism tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_push_delivers_subscribed_variables() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x"], "interval_ms": 0 }}),
    ).await;

    // Wait past the push throttle so the next broadcast isn't skipped
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "x".into(), value: "42".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "y".into(), value: "99".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 10, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");
    assert_eq!(push["cycle"], 10);
    let vars = push["variables"].as_array().unwrap();
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0]["name"], "x");
    assert_eq!(vars[0]["value"], "42");
}

#[tokio::test]
async fn test_push_stops_after_unsubscribe() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![VariableValue { name: "x".into(), value: "1".into(), var_type: "INT".into(), forced: false }],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");

    // Unsubscribe
    request(
        &mut ws,
        json!({ "method": "unsubscribe", "params": { "variables": ["x"] }}),
    ).await;

    // Push another update — client should still receive cycle stats but no
    // variable data (cycle stats always flow so the monitor panel stays live).
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.update_variables(
        vec![VariableValue { name: "x".into(), value: "2".into(), var_type: "INT".into(), forced: false }],
        CycleInfoData { cycle_count: 2, ..Default::default() },
    );

    let timeout = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
    if let Ok(Some(Ok(msg))) = timeout {
        let json: serde_json::Value = serde_json::from_str(&msg.to_string()).unwrap();
        // Should get a variableUpdate with cycle stats but no variable values
        assert_eq!(json["type"], "variableUpdate");
        let vars = json["variables"].as_array().expect("variables array");
        assert!(vars.is_empty(), "Should not receive variable data after unsubscribe, got: {vars:?}");
    }
    // Either timeout (no push) or empty-variables push are both acceptable
}

#[tokio::test]
async fn test_push_includes_forced_flag() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![VariableValue { name: "x".into(), value: "999".into(), var_type: "INT".into(), forced: true }],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["variables"][0]["forced"], true);
}

#[tokio::test]
async fn test_has_subscribers() {
    let (handle, addr) = start_server().await;

    // No clients connected yet
    assert!(!handle.has_subscribers());

    // Connect a client and subscribe
    let mut ws = connect(&addr).await;
    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["x"], "interval_ms": 0 }}),
    ).await;

    // Now there's a subscriber (the push task subscribed to the broadcast)
    assert!(handle.has_subscribers());

    // Disconnect
    ws.close(None).await.ok();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Subscriber gone (push task exited)
    assert!(!handle.has_subscribers());
}

// ── MonitorHandle unit tests ────────────────────────────────────────────

#[test]
fn test_handle_set_catalog() {
    let handle = MonitorHandle::new();
    handle.set_catalog(vec![
        CatalogEntry { name: "a".into(), var_type: "INT".into() },
    ]);
    assert_eq!(handle.state().read().unwrap().catalog.len(), 1);
}

#[test]
fn test_handle_update_variables() {
    let handle = MonitorHandle::new();
    handle.update_variables(
        vec![VariableValue { name: "x".into(), value: "42".into(), var_type: "INT".into(), forced: false }],
        CycleInfoData { cycle_count: 1, last_cycle_us: 10, min_cycle_us: 10, max_cycle_us: 10, avg_cycle_us: 10, ..Default::default() },
    );

    let st = handle.state().read().unwrap();
    assert_eq!(st.variables.get("x").unwrap().value, "42");
    assert_eq!(st.cycle_info.cycle_count, 1);
}

#[test]
fn test_handle_forced_variables() {
    let handle = MonitorHandle::new();
    handle.state().write().unwrap().forced_variables.insert("out".into(), st_ir::Value::Int(100));

    let forced = handle.peek_forced_variables();
    assert_eq!(forced.get("out"), Some(&st_ir::Value::Int(100)));

    let taken = handle.take_forced_variables();
    assert_eq!(taken.get("out"), Some(&st_ir::Value::Int(100)));
    assert!(handle.peek_forced_variables().is_empty());
}

// ── Protocol serialization tests ────────────────────────────────────────

#[test]
fn test_serialize_subscribe() {
    let req = MonitorRequest::Subscribe(SubscribeParams {
        variables: vec!["counter".into()],
        interval_ms: 100,
    });
    let json = serde_json::to_string(&req).unwrap();
    let parsed: MonitorRequest = serde_json::from_str(&json).unwrap();
    if let MonitorRequest::Subscribe(p) = parsed {
        assert_eq!(p.variables, vec!["counter"]);
        assert_eq!(p.interval_ms, 100);
    } else {
        panic!("Wrong variant");
    }
}

#[test]
fn test_serialize_variable_update() {
    let msg = MonitorMessage::VariableUpdate(VariableUpdateData {
        cycle: 42,
        last_cycle_us: 100,
        min_cycle_us: 50,
        max_cycle_us: 200,
        avg_cycle_us: 110,
        variables: vec![VariableValue {
            name: "x".into(), value: "10".into(), var_type: "INT".into(), forced: false,
        }],
        ..Default::default()
    });
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: MonitorMessage = serde_json::from_str(&json).unwrap();
    if let MonitorMessage::VariableUpdate(data) = parsed {
        assert_eq!(data.cycle, 42);
        assert_eq!(data.variables[0].name, "x");
    } else {
        panic!("Wrong variant");
    }
}

#[test]
fn test_serialize_catalog() {
    let msg = MonitorMessage::Catalog(CatalogData {
        variables: vec![CatalogEntry { name: "a".into(), var_type: "INT".into() }],
    });
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: MonitorMessage = serde_json::from_str(&json).unwrap();
    if let MonitorMessage::Catalog(data) = parsed {
        assert_eq!(data.variables.len(), 1);
        assert_eq!(data.variables[0].name, "a");
    } else {
        panic!("Wrong variant");
    }
}

// ── Compound-type prefix watch tests ──────────────────────────────────

#[tokio::test]
async fn test_subscribe_fb_parent_pushes_all_fields() {
    // Subscribing to a parent name like "Main.filler" should push all
    // descendant fields: "Main.filler.cmd", "Main.filler.state", etc.
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.filler"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.filler.cmd".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.state".into(), value: "2".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.filler.counter.Q".into(), value: "FALSE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.counter.CV".into(), value: "3".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.other".into(), value: "99".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");
    let vars = push["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();

    // All Main.filler.* descendants should be included
    assert!(names.contains(&"Main.filler.cmd"), "Should include Main.filler.cmd: {names:?}");
    assert!(names.contains(&"Main.filler.state"), "Should include Main.filler.state: {names:?}");
    assert!(names.contains(&"Main.filler.counter.Q"), "Should include nested Main.filler.counter.Q: {names:?}");
    assert!(names.contains(&"Main.filler.counter.CV"), "Should include nested Main.filler.counter.CV: {names:?}");
    // Unrelated variable should NOT be included
    assert!(!names.contains(&"Main.other"), "Should NOT include Main.other: {names:?}");
    assert_eq!(vars.len(), 4, "Expected exactly 4 descendant fields, got {}: {names:?}", vars.len());
}

#[tokio::test]
async fn test_subscribe_array_parent_pushes_elements() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.arr"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.arr[1]".into(), value: "10".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.arr[2]".into(), value: "20".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.arr[3]".into(), value: "30".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.total".into(), value: "60".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");
    let vars = push["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();

    assert!(names.contains(&"Main.arr[1]"), "Should include Main.arr[1]: {names:?}");
    assert!(names.contains(&"Main.arr[2]"), "Should include Main.arr[2]: {names:?}");
    assert!(names.contains(&"Main.arr[3]"), "Should include Main.arr[3]: {names:?}");
    assert!(!names.contains(&"Main.total"), "Should NOT include Main.total: {names:?}");
}

#[tokio::test]
async fn test_subscribe_mixed_scalar_and_compound() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.counter", "Main.fb"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.counter".into(), value: "42".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.fb.x".into(), value: "1".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.fb.y".into(), value: "2".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.other".into(), value: "99".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    let vars = push["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();

    assert!(names.contains(&"Main.counter"), "Should include scalar Main.counter: {names:?}");
    assert!(names.contains(&"Main.fb.x"), "Should include Main.fb.x: {names:?}");
    assert!(names.contains(&"Main.fb.y"), "Should include Main.fb.y: {names:?}");
    assert!(!names.contains(&"Main.other"), "Should NOT include Main.other: {names:?}");
    assert_eq!(vars.len(), 3);
}

#[tokio::test]
async fn test_read_fb_parent_returns_all_fields() {
    // A read request for "Main.filler" should return all descendant fields.
    let (handle, addr) = start_server().await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.filler.cmd".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.state".into(), value: "5".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.scalar".into(), value: "1".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let mut ws = connect(&addr).await;
    let resp = request(
        &mut ws,
        json!({ "method": "read", "params": { "variables": ["Main.filler"] }}),
    ).await;

    assert_eq!(resp["success"], true);
    let data = resp["data"].as_array().unwrap();
    let names: Vec<&str> = data.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(names.contains(&"Main.filler.cmd"), "Read should include Main.filler.cmd: {names:?}");
    assert!(names.contains(&"Main.filler.state"), "Read should include Main.filler.state: {names:?}");
    assert!(!names.contains(&"Main.scalar"), "Read should NOT include Main.scalar: {names:?}");
}

#[tokio::test]
async fn test_push_includes_watch_tree_for_compound_subscription() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.fb"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.fb".into(), value: "".into(), var_type: "Outer".into(), forced: false },
            VariableValue { name: "Main.fb.cmd".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.fb.state".into(), value: "5".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.fb.sub.x".into(), value: "3".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.fb.sub.y".into(), value: "6".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    assert_eq!(push["type"], "variableUpdate");

    // watch_tree must be present and well-structured
    let tree = push["watch_tree"].as_array().expect("watch_tree must be an array");
    assert_eq!(tree.len(), 1, "One root node for Main.fb");

    let root = &tree[0];
    assert_eq!(root["name"], "Main.fb");
    assert_eq!(root["fullPath"], "Main.fb");
    assert_eq!(root["kind"], "fb");
    assert_eq!(root["type"], "Outer");

    // Root must have children (cmd, state, sub)
    let children = root["children"].as_array().expect("Root must have children");
    let child_names: Vec<&str> = children.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(child_names.contains(&"cmd"), "Children should include cmd: {child_names:?}");
    assert!(child_names.contains(&"state"), "Children should include state: {child_names:?}");
    assert!(child_names.contains(&"sub"), "Children should include sub: {child_names:?}");

    // cmd is a leaf with fullPath and value
    let cmd = children.iter().find(|c| c["name"].as_str() == Some("cmd")).unwrap();
    assert_eq!(cmd["fullPath"], "Main.fb.cmd");
    assert_eq!(cmd["value"], "TRUE");
    assert_eq!(cmd["type"], "BOOL");
    assert_eq!(cmd["kind"], "scalar");

    // sub is a nested compound node with its own children
    let sub = children.iter().find(|c| c["name"].as_str() == Some("sub")).unwrap();
    assert!(sub["children"].as_array().is_some(), "sub should have children");
    let sub_children = sub["children"].as_array().unwrap();
    let sub_child_names: Vec<&str> = sub_children.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(sub_child_names.contains(&"x"), "sub children: {sub_child_names:?}");
    assert!(sub_child_names.contains(&"y"), "sub children: {sub_child_names:?}");

    // x leaf has correct fullPath and value
    let x = sub_children.iter().find(|c| c["name"].as_str() == Some("x")).unwrap();
    assert_eq!(x["fullPath"], "Main.fb.sub.x");
    assert_eq!(x["value"], "3");
}

#[tokio::test]
async fn test_push_watch_tree_for_array() {
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.arr"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.arr".into(), value: "".into(), var_type: "ARRAY[0..2] OF INT".into(), forced: false },
            VariableValue { name: "Main.arr[0]".into(), value: "10".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.arr[1]".into(), value: "20".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.arr[2]".into(), value: "30".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    let tree = push["watch_tree"].as_array().expect("watch_tree must be present");
    assert_eq!(tree.len(), 1);

    let root = &tree[0];
    assert_eq!(root["name"], "Main.arr");
    assert_eq!(root["kind"], "array");
    assert_eq!(root["type"], "ARRAY[0..2] OF INT");

    let children = root["children"].as_array().unwrap();
    assert_eq!(children.len(), 3, "Array should have 3 children");

    // Each child has correct fullPath and value
    let el0 = children.iter().find(|c| c["name"].as_str() == Some("[0]")).unwrap();
    assert_eq!(el0["fullPath"], "Main.arr[0]");
    assert_eq!(el0["value"], "10");
    assert_eq!(el0["kind"], "scalar");

    let el2 = children.iter().find(|c| c["name"].as_str() == Some("[2]")).unwrap();
    assert_eq!(el2["fullPath"], "Main.arr[2]");
    assert_eq!(el2["value"], "30");
}

#[tokio::test]
async fn test_watch_tree_intermediate_nodes_have_unique_full_paths() {
    // Bug regression: intermediate nodes (nested FBs) had empty fullPath,
    // causing all siblings to expand/collapse together.
    let (handle, addr) = start_server().await;
    let mut ws = connect(&addr).await;

    request(
        &mut ws,
        json!({ "method": "subscribe", "params": { "variables": ["Main.fb"], "interval_ms": 0 }}),
    ).await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    handle.update_variables(
        vec![
            VariableValue { name: "Main.fb".into(), value: "".into(), var_type: "Outer".into(), forced: false },
            VariableValue { name: "Main.fb.counter.CU".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.fb.counter.CV".into(), value: "3".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.fb.edge.CLK".into(), value: "FALSE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.fb.edge.Q".into(), value: "TRUE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.fb.state".into(), value: "5".into(), var_type: "INT".into(), forced: false },
        ],
        CycleInfoData { cycle_count: 1, ..Default::default() },
    );

    let push = recv_nonempty(&mut ws).await;
    let tree = push["watch_tree"].as_array().unwrap();
    let root = &tree[0];
    let children = root["children"].as_array().unwrap();

    // "counter" and "edge" are intermediate nodes — they MUST have distinct fullPaths
    let counter = children.iter().find(|c| c["name"].as_str() == Some("counter")).unwrap();
    let edge = children.iter().find(|c| c["name"].as_str() == Some("edge")).unwrap();

    assert_eq!(counter["fullPath"], "Main.fb.counter", "counter must have fullPath");
    assert_eq!(edge["fullPath"], "Main.fb.edge", "edge must have fullPath");
    assert_ne!(counter["fullPath"], edge["fullPath"], "counter and edge must differ");

    // Their leaf children should also have correct fullPaths
    let counter_children = counter["children"].as_array().unwrap();
    let cu = counter_children.iter().find(|c| c["name"].as_str() == Some("CU")).unwrap();
    assert_eq!(cu["fullPath"], "Main.fb.counter.CU");

    let edge_children = edge["children"].as_array().unwrap();
    let clk = edge_children.iter().find(|c| c["name"].as_str() == Some("CLK")).unwrap();
    assert_eq!(clk["fullPath"], "Main.fb.edge.CLK");
}

#[test]
fn test_serialize_all_requests_roundtrip() {
    let requests = vec![
        MonitorRequest::Subscribe(SubscribeParams { variables: vec!["a".into()], interval_ms: 0 }),
        MonitorRequest::Unsubscribe(UnsubscribeParams { variables: vec!["a".into()] }),
        MonitorRequest::Read(ReadParams { variables: vec!["a".into()] }),
        MonitorRequest::Write(WriteParams { variable: "a".into(), value: json!(42) }),
        MonitorRequest::Force(ForceParams { variable: "a".into(), value: json!(true) }),
        MonitorRequest::Unforce(UnforceParams { variable: "a".into() }),
        MonitorRequest::GetCycleInfo,
        MonitorRequest::GetCatalog,
        MonitorRequest::ResetStats,
        MonitorRequest::OnlineChange(OnlineChangeParams { source: "test".into() }),
    ];

    for req in &requests {
        let json = serde_json::to_string(req).unwrap();
        let parsed: MonitorRequest = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2, "Round-trip failed");
    }
}
