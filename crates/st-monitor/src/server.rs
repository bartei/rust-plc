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
            let tx = update_tx.clone();
            tokio::spawn(handle_client(stream, state, update_rx, tx));
        }
    });

    Ok(local_addr)
}

/// Per-client WebSocket session.
async fn handle_client(
    stream: tokio::net::TcpStream,
    state: Arc<RwLock<MonitorState>>,
    mut update_rx: broadcast::Receiver<()>,
    update_tx: broadcast::Sender<()>,
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
        // Initialize to the past so the first push is never throttled.
        let mut last_push = Instant::now() - Duration::from_secs(1);
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

            let (ci, vars, tree) = {
                let st = push_state.read().unwrap();
                let ci = st.cycle_info.clone();
                let vars: Vec<VariableValue> = collect_watched_variables(&subs, &st.variables);
                let forced_set: HashSet<String> = st.forced_variables.keys()
                    .map(|k| k.to_uppercase())
                    .collect();
                let tree = build_watch_tree(&subs, &st.variables, &forced_set);
                (ci, vars, tree)
            };
            drop(subs);

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
                watch_tree: tree,
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

        let response = handle_request(request, &state, &subscriptions, &update_tx).await;
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
    update_tx: &broadcast::Sender<()>,
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
            let count = subs.len();
            drop(subs);
            // Trigger immediate push so the client gets current values
            // without waiting for the next engine cycle.
            let _ = update_tx.send(());
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::json!({ "subscribed": count })),
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
            let name_set: HashSet<String> = params.variables.into_iter().collect();
            let vars: Vec<VariableValue> = collect_watched_variables(&name_set, &st.variables);
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

/// Collect variables matching the subscription set from a HashMap.
/// Public API for external callers (e.g., target-agent WS push task).
pub fn collect_watched_variables_from_map(
    subscriptions: &HashSet<String>,
    variables: &HashMap<String, VariableValue>,
) -> Vec<VariableValue> {
    collect_watched_variables(subscriptions, variables)
}

/// Collect variables matching the subscription set.
/// Supports exact match AND prefix match: subscribing to "Main.filler"
/// also yields "Main.filler.cmd", "Main.filler.counter.Q", "Main.arr[1]", etc.
fn collect_watched_variables(
    subscriptions: &HashSet<String>,
    variables: &HashMap<String, VariableValue>,
) -> Vec<VariableValue> {
    if subscriptions.is_empty() {
        return Vec::new();
    }
    // Build upper-cased prefix sets once for efficient matching.
    let exact: HashSet<String> = subscriptions.iter().map(|s| s.to_uppercase()).collect();
    let prefixes: Vec<(String, String)> = subscriptions
        .iter()
        .map(|s| {
            let u = s.to_uppercase();
            (format!("{u}."), format!("{u}["))
        })
        .collect();

    variables
        .values()
        .filter(|v| {
            let vu = v.name.to_uppercase();
            exact.contains(&vu)
                || prefixes
                    .iter()
                    .any(|(dot, bracket)| vu.starts_with(dot) || vu.starts_with(bracket))
        })
        .cloned()
        .collect()
}

/// Build a `WatchNode` tree for each subscription root.
/// The tree is built entirely server-side so the widget never parses names.
pub fn build_watch_tree(
    subscriptions: &HashSet<String>,
    variables: &HashMap<String, VariableValue>,
    forced: &HashSet<String>,
) -> Vec<WatchNode> {
    let mut roots = Vec::new();
    for sub_name in subscriptions {
        let upper = sub_name.to_uppercase();
        let dot_prefix = format!("{upper}.");
        let bracket_prefix = format!("{upper}[");

        // Collect all descendants
        let mut descendants: Vec<&VariableValue> = variables
            .values()
            .filter(|v| {
                let vu = v.name.to_uppercase();
                vu.starts_with(&dot_prefix) || vu.starts_with(&bracket_prefix)
            })
            .collect();
        descendants.sort_by(|a, b| a.name.cmp(&b.name));

        // Look up the parent entry (if it exists in variables)
        let parent = variables.get(sub_name.as_str())
            .or_else(|| variables.values().find(|v| v.name.eq_ignore_ascii_case(sub_name)));

        if descendants.is_empty() {
            // Scalar — leaf node
            if let Some(v) = parent {
                roots.push(WatchNode {
                    name: v.name.clone(),
                    full_path: v.name.clone(),
                    kind: "scalar".to_string(),
                    var_type: v.var_type.clone(),
                    value: v.value.clone(),
                    forced: forced.contains(&v.name.to_uppercase()),
                    retain: v.retain,
                    persistent: v.persistent,
                    children: Vec::new(),
                });
            } else {
                roots.push(WatchNode {
                    name: sub_name.clone(),
                    full_path: sub_name.clone(),
                    kind: "scalar".to_string(),
                    var_type: String::new(),
                    value: String::new(),
                    forced: false,
                    retain: false,
                    persistent: false,
                    children: Vec::new(),
                });
            }
        } else {
            // Compound — build children tree
            let parent_type = parent.map(|v| v.var_type.as_str()).unwrap_or("");
            let parent_retain = parent.map(|v| v.retain).unwrap_or(false);
            let parent_persistent = parent.map(|v| v.persistent).unwrap_or(false);
            let kind = if parent_type.starts_with("ARRAY") {
                "array"
            } else if parent_type == "PROGRAM" {
                "program"
            } else {
                "fb"
            };
            let children = build_children_from_flat(sub_name, &descendants, forced);
            roots.push(WatchNode {
                name: sub_name.clone(),
                full_path: sub_name.clone(),
                kind: kind.to_string(),
                var_type: parent_type.to_string(),
                value: String::new(),
                forced: false,
                retain: parent_retain,
                persistent: parent_persistent,
                children,
            });
        }
    }
    roots.sort_by(|a, b| a.name.cmp(&b.name));
    roots
}

/// Build nested children from flat descendant list.
fn build_children_from_flat(
    parent_path: &str,
    descendants: &[&VariableValue],
    forced: &HashSet<String>,
) -> Vec<WatchNode> {
    use std::collections::BTreeMap;

    struct Node {
        full_path: String,
        var_type: String,
        value: String,
        forced: bool,
        retain: bool,
        persistent: bool,
        children: BTreeMap<String, Node>,
    }

    let parent_upper = parent_path.to_uppercase();
    let dot_prefix = format!("{parent_upper}.");
    let bracket_prefix = format!("{parent_upper}[");
    let mut root = BTreeMap::<String, Node>::new();

    for v in descendants {
        let vu = v.name.to_uppercase();
        let relative = if vu.starts_with(&dot_prefix) {
            &v.name[parent_path.len() + 1..]
        } else if vu.starts_with(&bracket_prefix) {
            &v.name[parent_path.len()..]
        } else {
            continue;
        };

        // Split on dots, but keep bracket segments intact
        let parts = split_path_segments(relative);
        if parts.is_empty() {
            continue;
        }

        #[allow(clippy::too_many_arguments)]
        fn insert_at(
            map: &mut BTreeMap<String, Node>,
            parts: &[&str],
            full_path: &str,
            var_type: &str,
            value: &str,
            is_forced: bool,
            retain: bool,
            persistent: bool,
        ) {
            if parts.is_empty() {
                return;
            }
            let key = parts[0].to_string();
            let node = map.entry(key).or_insert_with(|| Node {
                full_path: String::new(),
                var_type: String::new(),
                value: String::new(),
                forced: false,
                retain: false,
                persistent: false,
                children: BTreeMap::new(),
            });
            if parts.len() == 1 {
                node.full_path = full_path.to_string();
                node.var_type = var_type.to_string();
                node.value = value.to_string();
                node.forced = is_forced;
                node.retain = retain;
                node.persistent = persistent;
            } else {
                // Intermediate compound node — propagate parent's retain bits
                // so the badge appears on every level (e.g. on Main.fb, the FB
                // instance row, not just on Main.fb.counter, the leaf field).
                if retain { node.retain = true; }
                if persistent { node.persistent = true; }
                insert_at(
                    &mut node.children,
                    &parts[1..],
                    full_path,
                    var_type,
                    value,
                    is_forced,
                    retain,
                    persistent,
                );
            }
        }

        let is_forced = forced.contains(&v.name.to_uppercase());
        insert_at(
            &mut root,
            &parts,
            &v.name,
            &v.var_type,
            &v.value,
            is_forced,
            v.retain,
            v.persistent,
        );
    }

    fn to_nodes(map: &BTreeMap<String, Node>, parent_full_path: &str) -> Vec<WatchNode> {
        map.iter()
            .map(|(key, node)| {
                // Compute fullPath for intermediate nodes that don't have one
                let full_path = if !node.full_path.is_empty() {
                    node.full_path.clone()
                } else if key.starts_with('[') {
                    format!("{parent_full_path}{key}")
                } else if parent_full_path.is_empty() {
                    key.clone()
                } else {
                    format!("{parent_full_path}.{key}")
                };
                let children = to_nodes(&node.children, &full_path);
                let kind = if !children.is_empty() {
                    if key.starts_with('[') { "array" } else { "fb" }
                } else {
                    "scalar"
                };
                WatchNode {
                    name: key.clone(),
                    full_path,
                    kind: kind.to_string(),
                    var_type: node.var_type.clone(),
                    value: node.value.clone(),
                    forced: node.forced,
                    retain: node.retain,
                    persistent: node.persistent,
                    children,
                }
            })
            .collect()
    }

    to_nodes(&root, parent_path)
}

/// Split a relative path into segments, keeping bracket indices intact.
/// "counter.CV" → ["counter", "CV"]
/// "[0]" → ["[0]"]
/// "sub.x" → ["sub", "x"]
fn split_path_segments(path: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            if i > start {
                segments.push(&path[start..i]);
            }
            start = i + 1;
        } else if bytes[i] == b'[' && i > start {
            // Push what we have before the bracket, then the bracket segment
            segments.push(&path[start..i]);
            start = i;
        }
        i += 1;
    }
    if start < bytes.len() {
        segments.push(&path[start..]);
    }
    segments
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
