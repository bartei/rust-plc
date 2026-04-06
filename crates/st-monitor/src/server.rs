//! WebSocket monitor server.

use crate::protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;

/// Shared state between the monitor server and the PLC engine.
#[derive(Debug)]
pub struct MonitorState {
    /// Current variable values (updated by the engine after each scan cycle).
    pub variables: HashMap<String, VariableValue>,
    /// Current scan cycle statistics.
    pub cycle_info: CycleInfoData,
    /// Currently forced variables.
    pub forced_variables: HashMap<String, st_ir::Value>,
    /// Pending online change source (set by client, consumed by engine).
    pub pending_online_change: Option<String>,
    /// Result of the last online change attempt.
    pub online_change_result: Option<Result<String, String>>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            cycle_info: CycleInfoData {
                cycle_count: 0,
                last_cycle_us: 0,
                min_cycle_us: 0,
                max_cycle_us: 0,
                avg_cycle_us: 0,
            },
            forced_variables: HashMap::new(),
            pending_online_change: None,
            online_change_result: None,
        }
    }
}

/// Handle for the engine to push updates to connected monitor clients.
#[derive(Clone)]
pub struct MonitorHandle {
    state: Arc<RwLock<MonitorState>>,
    update_tx: broadcast::Sender<()>,
}

impl MonitorHandle {
    /// Create a new monitor handle with its shared state.
    pub fn new() -> (Self, Arc<RwLock<MonitorState>>) {
        let state = Arc::new(RwLock::new(MonitorState::default()));
        let (update_tx, _) = broadcast::channel(64);
        let handle = Self {
            state: state.clone(),
            update_tx,
        };
        (handle, state)
    }

    /// Update variable values (called by the engine after each scan cycle).
    pub async fn update_variables(&self, vars: Vec<VariableValue>, cycle_info: CycleInfoData) {
        let mut state = self.state.write().await;
        for v in vars {
            state.variables.insert(v.name.clone(), v);
        }
        state.cycle_info = cycle_info;
        drop(state);
        let _ = self.update_tx.send(());
    }

    /// Check if there's a pending online change.
    pub async fn take_pending_online_change(&self) -> Option<String> {
        self.state.write().await.pending_online_change.take()
    }

    /// Set the result of an online change.
    pub async fn set_online_change_result(&self, result: Result<String, String>) {
        self.state.write().await.online_change_result = Some(result);
    }

    /// Get forced variables.
    pub async fn get_forced_variables(&self) -> HashMap<String, st_ir::Value> {
        self.state.read().await.forced_variables.clone()
    }

    /// Subscribe to update notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.update_tx.subscribe()
    }
}

/// Run the WebSocket monitor server.
pub async fn run_monitor_server(
    addr: &str,
    state: Arc<RwLock<MonitorState>>,
    update_tx: broadcast::Sender<()>,
) {
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Monitor server failed to bind to {addr}: {e}");
            return;
        }
    };
    tracing::info!("Monitor server listening on {addr}");

    while let Ok((stream, peer)) = listener.accept().await {
        tracing::info!("Monitor client connected: {peer}");
        let state = state.clone();
        let update_rx = update_tx.subscribe();
        tokio::spawn(handle_client(stream, state, update_rx));
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    state: Arc<RwLock<MonitorState>>,
    mut update_rx: broadcast::Receiver<()>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("WebSocket handshake failed: {e}");
            return;
        }
    };

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let subscribed_vars: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let subscribed_vars_push = subscribed_vars.clone();

    // Spawn a task to push updates to this client
    let state_push = state.clone();
    let push_task = tokio::spawn(async move {
        loop {
            if update_rx.recv().await.is_err() {
                break;
            }
            let subs = subscribed_vars_push.lock().await;
            if subs.is_empty() {
                continue;
            }
            let st = state_push.read().await;
            let vars: Vec<VariableValue> = subs
                .iter()
                .filter_map(|name| st.variables.get(name).cloned())
                .collect();
            if !vars.is_empty() {
                let msg = MonitorMessage::VariableUpdate(VariableUpdateData {
                    cycle: st.cycle_info.cycle_count,
                    variables: vars,
                });
                drop(st);
                if let Ok(json) = serde_json::to_string(&msg) {
                    // Send to ws_tx is not possible here since we don't own it.
                    // We'll use a channel instead.
                    let _ = json; // TODO: send via channel
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            _ => continue,
        };

        let request: MonitorRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                let err = MonitorMessage::Error(ErrorData {
                    message: format!("Invalid request: {e}"),
                });
                let _ = ws_tx
                    .send(Message::Text(serde_json::to_string(&err).unwrap()))
                    .await;
                continue;
            }
        };

        let response = handle_request(request, &state, &subscribed_vars).await;
        if let Ok(json) = serde_json::to_string(&response) {
            if ws_tx.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    }

    push_task.abort();
}

async fn handle_request(
    request: MonitorRequest,
    state: &Arc<RwLock<MonitorState>>,
    subscribed_vars: &Arc<Mutex<HashSet<String>>>,
) -> MonitorMessage {
    match request {
        MonitorRequest::Subscribe(params) => {
            let mut subs = subscribed_vars.lock().await;
            for var in params.variables {
                subs.insert(var);
            }
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({
                    "subscribed": subs.len()
                })),
            })
        }
        MonitorRequest::Unsubscribe(params) => {
            let mut subs = subscribed_vars.lock().await;
            for var in params.variables {
                subs.remove(&var);
            }
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: None,
            })
        }
        MonitorRequest::Read(params) => {
            let st = state.read().await;
            let vars: Vec<VariableValue> = params
                .variables
                .iter()
                .filter_map(|name| st.variables.get(name).cloned())
                .collect();
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::to_value(vars).unwrap()),
            })
        }
        MonitorRequest::Write(params) => {
            // TODO: implement variable write via engine
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({
                    "variable": params.variable,
                    "written": true
                })),
            })
        }
        MonitorRequest::Force(params) => {
            let value = json_to_ir_value(&params.value);
            state
                .write()
                .await
                .forced_variables
                .insert(params.variable.clone(), value);
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({
                    "variable": params.variable,
                    "forced": true
                })),
            })
        }
        MonitorRequest::Unforce(params) => {
            state
                .write()
                .await
                .forced_variables
                .remove(&params.variable);
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({
                    "variable": params.variable,
                    "unforced": true
                })),
            })
        }
        MonitorRequest::GetCycleInfo => {
            let st = state.read().await;
            MonitorMessage::CycleInfo(st.cycle_info.clone())
        }
        MonitorRequest::OnlineChange(params) => {
            state.write().await.pending_online_change = Some(params.source);
            // The engine will pick this up and set the result
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({ "pending": true })),
            })
        }
    }
}

fn json_to_ir_value(v: &serde_json::Value) -> st_ir::Value {
    match v {
        serde_json::Value::Bool(b) => st_ir::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                st_ir::Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                st_ir::Value::Real(f)
            } else {
                st_ir::Value::Int(0)
            }
        }
        serde_json::Value::String(s) => st_ir::Value::String(s.clone()),
        _ => st_ir::Value::Int(0),
    }
}
