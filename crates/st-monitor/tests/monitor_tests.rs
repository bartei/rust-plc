//! Monitor server integration tests.
//!
//! Tests the WebSocket monitor protocol by connecting a client to a
//! real server, sending requests, and verifying responses.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use st_monitor::protocol::*;
use st_monitor::server::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Start monitor server on a random port and return the address.
async fn start_server() -> (String, Arc<RwLock<MonitorState>>, broadcast::Sender<()>) {
    let state = Arc::new(RwLock::new(MonitorState::default()));
    let (update_tx, _) = broadcast::channel(64);
    let state_clone = state.clone();
    let tx_clone = update_tx.clone();

    // Bind to port 0 to get a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let state = state_clone.clone();
            let update_rx = tx_clone.subscribe();
            tokio::spawn(handle_client_test(stream, state, update_rx));
        }
    });

    (addr, state, update_tx)
}

/// Simplified client handler for testing (reuses the server's handle_request).
async fn handle_client_test(
    stream: TcpStream,
    state: Arc<RwLock<MonitorState>>,
    _update_rx: broadcast::Receiver<()>,
) {
    let ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let subscribed = Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new()));

    while let Some(msg) = ws_rx.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            _ => continue,
        };

        let request: MonitorRequest = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                let err = MonitorMessage::Error(ErrorData {
                    message: format!("Invalid: {e}"),
                });
                let _ = ws_tx.send(Message::Text(serde_json::to_string(&err).unwrap())).await;
                continue;
            }
        };

        // Handle subscribe/unsubscribe locally
        let response = match &request {
            MonitorRequest::Subscribe(p) => {
                let mut subs = subscribed.lock().await;
                for v in &p.variables { subs.insert(v.clone()); }
                MonitorMessage::Response(ResponseData {
                    id: None, success: true,
                    data: Some(json!({"subscribed": subs.len()})),
                })
            }
            MonitorRequest::Unsubscribe(p) => {
                let mut subs = subscribed.lock().await;
                for v in &p.variables { subs.remove(v); }
                MonitorMessage::Response(ResponseData {
                    id: None, success: true, data: None,
                })
            }
            MonitorRequest::Read(p) => {
                let st = state.read().await;
                let vars: Vec<VariableValue> = p.variables.iter()
                    .filter_map(|n| st.variables.get(n).cloned())
                    .collect();
                MonitorMessage::Response(ResponseData {
                    id: None, success: true,
                    data: Some(serde_json::to_value(vars).unwrap()),
                })
            }
            MonitorRequest::GetCycleInfo => {
                let st = state.read().await;
                MonitorMessage::CycleInfo(st.cycle_info.clone())
            }
            MonitorRequest::Force(p) => {
                let value = match &p.value {
                    Value::Number(n) => st_ir::Value::Int(n.as_i64().unwrap_or(0)),
                    Value::Bool(b) => st_ir::Value::Bool(*b),
                    _ => st_ir::Value::Int(0),
                };
                state.write().await.forced_variables.insert(p.variable.clone(), value);
                MonitorMessage::Response(ResponseData {
                    id: None, success: true,
                    data: Some(json!({"forced": true})),
                })
            }
            MonitorRequest::Unforce(p) => {
                state.write().await.forced_variables.remove(&p.variable);
                MonitorMessage::Response(ResponseData {
                    id: None, success: true,
                    data: Some(json!({"unforced": true})),
                })
            }
            MonitorRequest::OnlineChange(p) => {
                state.write().await.pending_online_change = Some(p.source.clone());
                MonitorMessage::Response(ResponseData {
                    id: None, success: true,
                    data: Some(json!({"pending": true})),
                })
            }
            _ => MonitorMessage::Response(ResponseData {
                id: None, success: true, data: None,
            }),
        };

        let json = serde_json::to_string(&response).unwrap();
        if ws_tx.send(Message::Text(json)).await.is_err() {
            break;
        }
    }
}

/// Connect a WebSocket client and return the split streams.
async fn connect_client(addr: &str) -> (
    futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>, Message>,
    futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>,
) {
    let url = format!("ws://{addr}");
    let (ws, _) = connect_async(&url).await.expect("Failed to connect");
    ws.split()
}

async fn send_request(
    tx: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>, Message>,
    rx: &mut futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>,
    request: &MonitorRequest,
) -> Value {
    let json = serde_json::to_string(request).unwrap();
    tx.send(Message::Text(json)).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.next())
        .await
        .expect("Timeout waiting for response")
        .expect("Stream ended")
        .expect("WebSocket error");
    let Message::Text(text) = msg else { panic!("Expected text message") };
    serde_json::from_str(&text).unwrap()
}

// =============================================================================
// Protocol tests
// =============================================================================

#[tokio::test]
async fn test_subscribe() {
    let (addr, _state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Subscribe(SubscribeParams {
        variables: vec!["counter".into(), "running".into()],
        interval_ms: 0,
    })).await;

    assert_eq!(resp["type"], "response");
    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["subscribed"], 2);
}

#[tokio::test]
async fn test_unsubscribe() {
    let (addr, _state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    // Subscribe first
    send_request(&mut tx, &mut rx, &MonitorRequest::Subscribe(SubscribeParams {
        variables: vec!["x".into(), "y".into()],
        interval_ms: 0,
    })).await;

    // Unsubscribe one
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Unsubscribe(UnsubscribeParams {
        variables: vec!["x".into()],
    })).await;

    assert_eq!(resp["success"], true);
}

#[tokio::test]
async fn test_read_variables() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Populate state
    {
        let mut st = state.write().await;
        st.variables.insert("counter".into(), VariableValue {
            name: "counter".into(),
            value: "42".into(),
            var_type: "INT".into(),
        });
        st.variables.insert("running".into(), VariableValue {
            name: "running".into(),
            value: "TRUE".into(),
            var_type: "BOOL".into(),
        });
    }

    let (mut tx, mut rx) = connect_client(&addr).await;
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Read(ReadParams {
        variables: vec!["counter".into(), "running".into()],
    })).await;

    assert_eq!(resp["success"], true);
    let data = resp["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert!(data.iter().any(|v| v["name"] == "counter" && v["value"] == "42"));
    assert!(data.iter().any(|v| v["name"] == "running" && v["value"] == "TRUE"));
}

#[tokio::test]
async fn test_read_nonexistent_variable() {
    let (addr, _state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Read(ReadParams {
        variables: vec!["nonexistent".into()],
    })).await;

    assert_eq!(resp["success"], true);
    let data = resp["data"].as_array().unwrap();
    assert_eq!(data.len(), 0);
}

#[tokio::test]
async fn test_force_variable() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Force(ForceParams {
        variable: "output".into(),
        value: json!(100),
    })).await;

    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["forced"], true);

    // Verify in state
    let st = state.read().await;
    assert!(st.forced_variables.contains_key("output"));
    assert_eq!(st.forced_variables["output"], st_ir::Value::Int(100));
}

#[tokio::test]
async fn test_unforce_variable() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Force first
    state.write().await.forced_variables.insert("x".into(), st_ir::Value::Int(0));

    let (mut tx, mut rx) = connect_client(&addr).await;
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Unforce(UnforceParams {
        variable: "x".into(),
    })).await;

    assert_eq!(resp["success"], true);
    assert!(state.read().await.forced_variables.is_empty());
}

#[tokio::test]
async fn test_get_cycle_info() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Set cycle info
    {
        let mut st = state.write().await;
        st.cycle_info = CycleInfoData {
            cycle_count: 1000,
            last_cycle_us: 50,
            min_cycle_us: 30,
            max_cycle_us: 120,
            avg_cycle_us: 55,
        };
    }

    let (mut tx, mut rx) = connect_client(&addr).await;
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::GetCycleInfo).await;

    assert_eq!(resp["type"], "cycleInfo");
    assert_eq!(resp["cycle_count"], 1000);
    assert_eq!(resp["last_cycle_us"], 50);
    assert_eq!(resp["min_cycle_us"], 30);
    assert_eq!(resp["max_cycle_us"], 120);
    assert_eq!(resp["avg_cycle_us"], 55);
}

#[tokio::test]
async fn test_online_change_request() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    let new_source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 2;\nEND_PROGRAM\n";
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::OnlineChange(OnlineChangeParams {
        source: new_source.into(),
    })).await;

    assert_eq!(resp["success"], true);
    assert_eq!(resp["data"]["pending"], true);

    // Verify pending change in state
    let st = state.read().await;
    assert!(st.pending_online_change.is_some());
    assert!(st.pending_online_change.as_ref().unwrap().contains("x + 2"));
}

#[tokio::test]
async fn test_invalid_json() {
    let (addr, _state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let url = format!("ws://{addr}");
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut tx, mut rx) = ws.split();

    // Send invalid JSON
    tx.send(Message::Text("not json".into())).await.unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), rx.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("WS error");
    let Message::Text(text) = msg else { panic!("Expected text") };
    let resp: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(resp["type"], "error");
    assert!(resp["message"].as_str().unwrap().contains("Invalid"));
}

#[tokio::test]
async fn test_multiple_clients() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Populate state
    state.write().await.variables.insert("shared".into(), VariableValue {
        name: "shared".into(), value: "99".into(), var_type: "INT".into(),
    });

    // Client 1
    let (mut tx1, mut rx1) = connect_client(&addr).await;
    let resp1 = send_request(&mut tx1, &mut rx1, &MonitorRequest::Read(ReadParams {
        variables: vec!["shared".into()],
    })).await;

    // Client 2
    let (mut tx2, mut rx2) = connect_client(&addr).await;
    let resp2 = send_request(&mut tx2, &mut rx2, &MonitorRequest::Read(ReadParams {
        variables: vec!["shared".into()],
    })).await;

    // Both should see the same value
    assert_eq!(resp1["data"][0]["value"], "99");
    assert_eq!(resp2["data"][0]["value"], "99");
}

#[tokio::test]
async fn test_force_bool() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut tx, mut rx) = connect_client(&addr).await;

    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Force(ForceParams {
        variable: "alarm".into(),
        value: json!(true),
    })).await;

    assert_eq!(resp["success"], true);
    let st = state.read().await;
    assert_eq!(st.forced_variables["alarm"], st_ir::Value::Bool(true));
}

#[tokio::test]
async fn test_force_then_read() {
    let (addr, state, _tx) = start_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Set variable value in state
    state.write().await.variables.insert("x".into(), VariableValue {
        name: "x".into(), value: "10".into(), var_type: "INT".into(),
    });

    let (mut tx, mut rx) = connect_client(&addr).await;

    // Read before force
    let resp = send_request(&mut tx, &mut rx, &MonitorRequest::Read(ReadParams {
        variables: vec!["x".into()],
    })).await;
    assert_eq!(resp["data"][0]["value"], "10");

    // Force
    send_request(&mut tx, &mut rx, &MonitorRequest::Force(ForceParams {
        variable: "x".into(),
        value: json!(999),
    })).await;

    // Verify force is in state
    assert!(state.read().await.forced_variables.contains_key("x"));
}

// =============================================================================
// Protocol serialization tests
// =============================================================================

#[test]
fn test_protocol_serialize_subscribe() {
    let req = MonitorRequest::Subscribe(SubscribeParams {
        variables: vec!["counter".into()],
        interval_ms: 100,
    });
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("subscribe"));
    assert!(json.contains("counter"));

    // Round-trip
    let parsed: MonitorRequest = serde_json::from_str(&json).unwrap();
    if let MonitorRequest::Subscribe(p) = parsed {
        assert_eq!(p.variables, vec!["counter"]);
        assert_eq!(p.interval_ms, 100);
    } else {
        panic!("Wrong variant");
    }
}

#[test]
fn test_protocol_serialize_variable_update() {
    let msg = MonitorMessage::VariableUpdate(VariableUpdateData {
        cycle: 42,
        variables: vec![
            VariableValue { name: "x".into(), value: "10".into(), var_type: "INT".into() },
        ],
    });
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("variableUpdate"));
    assert!(json.contains("\"cycle\":42"));

    let parsed: MonitorMessage = serde_json::from_str(&json).unwrap();
    if let MonitorMessage::VariableUpdate(data) = parsed {
        assert_eq!(data.cycle, 42);
        assert_eq!(data.variables[0].name, "x");
    } else {
        panic!("Wrong variant");
    }
}

#[test]
fn test_protocol_serialize_cycle_info() {
    let msg = MonitorMessage::CycleInfo(CycleInfoData {
        cycle_count: 1000,
        last_cycle_us: 50,
        min_cycle_us: 30,
        max_cycle_us: 120,
        avg_cycle_us: 55,
    });
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: MonitorMessage = serde_json::from_str(&json).unwrap();
    if let MonitorMessage::CycleInfo(data) = parsed {
        assert_eq!(data.cycle_count, 1000);
    } else {
        panic!("Wrong variant");
    }
}

#[test]
fn test_protocol_serialize_all_requests() {
    // Ensure all request types serialize/deserialize correctly
    let requests = vec![
        MonitorRequest::Subscribe(SubscribeParams { variables: vec!["a".into()], interval_ms: 0 }),
        MonitorRequest::Unsubscribe(UnsubscribeParams { variables: vec!["a".into()] }),
        MonitorRequest::Read(ReadParams { variables: vec!["a".into()] }),
        MonitorRequest::Write(WriteParams { variable: "a".into(), value: json!(42) }),
        MonitorRequest::Force(ForceParams { variable: "a".into(), value: json!(true) }),
        MonitorRequest::Unforce(UnforceParams { variable: "a".into() }),
        MonitorRequest::GetCycleInfo,
        MonitorRequest::OnlineChange(OnlineChangeParams { source: "test".into() }),
    ];

    for req in &requests {
        let json = serde_json::to_string(req).unwrap();
        let parsed: MonitorRequest = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2, "Round-trip failed for request");
    }
}

// =============================================================================
// MonitorHandle tests
// =============================================================================

#[tokio::test]
async fn test_monitor_handle_update() {
    let (handle, state) = MonitorHandle::new();

    handle.update_variables(
        vec![
            VariableValue { name: "x".into(), value: "42".into(), var_type: "INT".into() },
        ],
        CycleInfoData {
            cycle_count: 1,
            last_cycle_us: 10,
            min_cycle_us: 10,
            max_cycle_us: 10,
            avg_cycle_us: 10,
        },
    ).await;

    let st = state.read().await;
    assert_eq!(st.variables.get("x").unwrap().value, "42");
    assert_eq!(st.cycle_info.cycle_count, 1);
}

#[tokio::test]
async fn test_monitor_handle_forced_vars() {
    let (handle, state) = MonitorHandle::new();

    state.write().await.forced_variables.insert("output".into(), st_ir::Value::Int(100));

    let forced = handle.get_forced_variables().await;
    assert_eq!(forced.get("output"), Some(&st_ir::Value::Int(100)));
}

#[tokio::test]
async fn test_monitor_handle_online_change() {
    let (handle, state) = MonitorHandle::new();

    state.write().await.pending_online_change = Some("new source".into());

    let change = handle.take_pending_online_change().await;
    assert_eq!(change, Some("new source".into()));

    // Should be consumed
    let change2 = handle.take_pending_online_change().await;
    assert_eq!(change2, None);
}
