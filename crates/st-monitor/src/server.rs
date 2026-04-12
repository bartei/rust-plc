//! WebSocket monitor server — reusable by both st-target-agent and st-dap.
//!
//! Architecture per client:
//!   Push task (cycle broadcast → filter by subscriptions) ─→ mpsc ─→ Writer task ─→ WS
//!   Reader task (WS → parse request → handle) ─────────────→ mpsc ─→ Writer task ─→ WS

use crate::protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::Message;

/// Shared state between the monitor server and the PLC engine.
/// Uses `std::sync::RwLock` so both synchronous (DAP scan cycle) and
/// async (WS handler) code can access it.
#[derive(Debug, Default)]
pub struct MonitorState {
    /// Current variable values, keyed by name.
    pub variables: HashMap<String, VariableValue>,
    /// Variable catalog (names + types). Set once when engine starts.
    pub catalog: Vec<CatalogEntry>,
    /// Current scan cycle statistics.
    pub cycle_info: CycleInfoData,
    /// Currently forced variables — WS clients write here, engine reads
    /// and applies on the next cycle.
    pub forced_variables: HashMap<String, st_ir::Value>,
    /// Pending online change source (set by client, consumed by engine).
    pub pending_online_change: Option<String>,
    /// Flag: WS client requested a stats reset. Engine clears it after applying.
    pub reset_stats_requested: bool,
}



/// Handle for the engine to push updates and read force commands.
#[derive(Clone)]
pub struct MonitorHandle {
    state: Arc<RwLock<MonitorState>>,
    update_tx: broadcast::Sender<()>,
}

impl MonitorHandle {
    /// Create a new monitor handle with its shared state.
    pub fn new() -> Self {
        let state = Arc::new(RwLock::new(MonitorState::default()));
        let (update_tx, _) = broadcast::channel(64);
        Self { state, update_tx }
    }

    /// Get a reference to the shared state (for the server).
    pub fn state(&self) -> &Arc<RwLock<MonitorState>> {
        &self.state
    }

    /// Get the broadcast sender (for the server).
    pub fn update_sender(&self) -> &broadcast::Sender<()> {
        &self.update_tx
    }

    /// Set the variable catalog (called once when the engine starts).
    pub fn set_catalog(&self, catalog: Vec<CatalogEntry>) {
        self.state.write().unwrap().catalog = catalog;
    }

    /// Update variable values and cycle info (called by the engine after each cycle).
    /// Only snapshots when there are active subscribers (receiver_count > 0).
    pub fn update_variables(&self, vars: Vec<VariableValue>, cycle_info: CycleInfoData) {
        {
            let mut state = self.state.write().unwrap();
            state.variables.clear();
            for v in vars {
                state.variables.insert(v.name.clone(), v);
            }
            state.cycle_info = cycle_info;
        }
        let _ = self.update_tx.send(());
    }

    /// Check if any clients are listening (use to gate expensive snapshots).
    pub fn has_subscribers(&self) -> bool {
        self.update_tx.receiver_count() > 0
    }

    /// Read and clear forced variables (called by the engine between cycles).
    pub fn take_forced_variables(&self) -> HashMap<String, st_ir::Value> {
        let mut state = self.state.write().unwrap();
        std::mem::take(&mut state.forced_variables)
    }

    /// Read forced variables without clearing (for snapshot forced flag).
    pub fn peek_forced_variables(&self) -> HashMap<String, st_ir::Value> {
        self.state.read().unwrap().forced_variables.clone()
    }

    /// Subscribe to cycle update notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.update_tx.subscribe()
    }

    /// Check and clear the reset-stats flag.
    pub fn take_reset_stats(&self) -> bool {
        let mut state = self.state.write().unwrap();
        let was = state.reset_stats_requested;
        state.reset_stats_requested = false;
        was
    }

    /// Check for a pending online change.
    pub fn take_pending_online_change(&self) -> Option<String> {
        self.state.write().unwrap().pending_online_change.take()
    }
}

impl Default for MonitorHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the WebSocket monitor server on the given address.
/// Returns the local address the server bound to (useful with port 0).
pub async fn run_monitor_server(
    addr: &str,
    handle: MonitorHandle,
) -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!("Monitor WS server listening on {local_addr}");

    let state = handle.state().clone();
    let update_tx = handle.update_sender().clone();

    tokio::spawn(async move {
        while let Ok((stream, peer)) = listener.accept().await {
            tracing::info!("Monitor WS: client connected from {peer}");
            let state = state.clone();
            let update_rx = update_tx.subscribe();
            tokio::spawn(handle_client(stream, state, update_rx));
        }
    });

    Ok(local_addr)
}

/// Per-client WebSocket session.
async fn handle_client(
    stream: tokio::net::TcpStream,
    state: Arc<RwLock<MonitorState>>,
    mut update_rx: broadcast::Receiver<()>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("Monitor WS: handshake failed: {e}");
            return;
        }
    };

    let (ws_tx, mut ws_rx) = ws_stream.split();

    // Outbound channel — both the push task and request handler send here,
    // the writer task drains to the WebSocket.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<String>(64);

    // Writer task: drains outbound channel to the WebSocket write half.
    let writer_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        while let Some(json) = out_rx.recv().await {
            if ws_tx.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Per-client subscription set
    let subscriptions: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Push task: wakes on cycle broadcast, filters by subscriptions, sends
    // variable updates with cycle stats included in every message.
    let push_out = out_tx.clone();
    let push_subs = subscriptions.clone();
    let push_state = state.clone();
    let push_task = tokio::spawn(async move {
        let mut last_push = Instant::now();
        let throttle = Duration::from_millis(50);
        let mut push_count: u64 = 0;

        loop {
            if update_rx.recv().await.is_err() {
                break;
            }

            let now = Instant::now();
            if now.duration_since(last_push) < throttle {
                continue;
            }

            let subs = push_subs.lock().await;
            if subs.is_empty() {
                continue;
            }

            let (ci, vars, snapshot_len) = {
                let st = push_state.read().unwrap();
                let ci = st.cycle_info.clone();
                let vars: Vec<VariableValue> = subs
                    .iter()
                    .filter_map(|name| st.variables.get(name).cloned())
                    .collect();
                let len = st.variables.len();
                (ci, vars, len)
            };
            drop(subs);

            if vars.is_empty() {
                if push_count == 0 {
                    tracing::debug!(
                        "Monitor WS: push skip — subscriptions present but 0 matched \
                         ({snapshot_len} vars in snapshot)"
                    );
                }
                continue;
            }

            let msg = MonitorMessage::VariableUpdate(VariableUpdateData {
                cycle: ci.cycle_count,
                last_cycle_us: ci.last_cycle_us,
                min_cycle_us: ci.min_cycle_us,
                max_cycle_us: ci.max_cycle_us,
                avg_cycle_us: ci.avg_cycle_us,
                target_cycle_us: ci.target_cycle_us,
                last_period_us: ci.last_period_us,
                min_period_us: ci.min_period_us,
                max_period_us: ci.max_period_us,
                jitter_max_us: ci.jitter_max_us,
                variables: vars.clone(),
            });
            if let Ok(json) = serde_json::to_string(&msg) {
                if push_out.send(json).await.is_err() {
                    break; // writer closed
                }
                push_count += 1;
                if push_count == 1 {
                    tracing::info!(
                        "Monitor WS: first push — {} vars at cycle {}",
                        vars.len(),
                        ci.cycle_count
                    );
                } else if push_count % 200 == 0 {
                    tracing::debug!(
                        "Monitor WS: push #{push_count} — {} vars at cycle {}",
                        vars.len(),
                        ci.cycle_count
                    );
                }
                last_push = Instant::now();
            }
        }
    });

    // Reader loop: parse incoming requests, handle them, send responses.
    while let Some(msg) = ws_rx.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => {
                tracing::info!("Monitor WS: client sent Close");
                break;
            }
            Err(e) => {
                tracing::debug!("Monitor WS: read error: {e}");
                break;
            }
            _ => continue,
        };

        tracing::debug!("Monitor WS: recv ← {text}");

        let request: MonitorRequest = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Monitor WS: invalid JSON: {e}");
                let err = MonitorMessage::Error(ErrorData {
                    message: format!("Invalid request: {e}"),
                });
                if let Ok(json) = serde_json::to_string(&err) {
                    let _ = out_tx.send(json).await;
                }
                continue;
            }
        };

        let response = handle_request(request, &state, &subscriptions).await;
        if let Ok(json) = serde_json::to_string(&response) {
            tracing::debug!("Monitor WS: send → {json}");
            if out_tx.send(json).await.is_err() {
                break;
            }
        }
    }

    tracing::info!("Monitor WS: client session ended");
    push_task.abort();
    writer_task.abort();
}

/// Handle a single client request.
async fn handle_request(
    request: MonitorRequest,
    state: &Arc<RwLock<MonitorState>>,
    subscriptions: &Arc<Mutex<HashSet<String>>>,
) -> MonitorMessage {
    match request {
        MonitorRequest::Subscribe(params) => {
            let mut subs = subscriptions.lock().await;
            for var in &params.variables {
                subs.insert(var.clone());
            }
            tracing::info!(
                "Monitor WS: subscribe {:?} — total {}",
                params.variables,
                subs.len()
            );
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({ "subscribed": subs.len() })),
            })
        }
        MonitorRequest::Unsubscribe(params) => {
            let mut subs = subscriptions.lock().await;
            for var in &params.variables {
                subs.remove(var);
            }
            tracing::info!(
                "Monitor WS: unsubscribe {:?} — {} remaining",
                params.variables,
                subs.len()
            );
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: None,
            })
        }
        MonitorRequest::Read(params) => {
            let st = state.read().unwrap();
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
        MonitorRequest::GetCatalog => {
            let st = state.read().unwrap();
            tracing::info!("Monitor WS: getCatalog — {} entries", st.catalog.len());
            MonitorMessage::Catalog(CatalogData {
                variables: st.catalog.clone(),
            })
        }
        MonitorRequest::GetCycleInfo => {
            let st = state.read().unwrap();
            MonitorMessage::CycleInfo(st.cycle_info.clone())
        }
        MonitorRequest::Force(params) => {
            let value = json_to_ir_value(&params.value);
            tracing::info!(
                "Monitor WS: force {} = {:?}",
                params.variable,
                params.value
            );
            state
                .write()
                .unwrap()
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
            tracing::info!("Monitor WS: unforce {}", params.variable);
            state
                .write()
                .unwrap()
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
        MonitorRequest::ResetStats => {
            tracing::info!("Monitor WS: resetStats requested");
            state.write().unwrap().reset_stats_requested = true;
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: None,
            })
        }
        MonitorRequest::Write(_) => MonitorMessage::Error(ErrorData {
            message: "Not implemented".to_string(),
        }),
        MonitorRequest::OnlineChange(params) => {
            state.write().unwrap().pending_online_change = Some(params.source);
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
