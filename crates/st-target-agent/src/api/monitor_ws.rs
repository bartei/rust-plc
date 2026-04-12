//! WebSocket endpoint for real-time PLC variable monitoring.
//!
//! Protocol (JSON over WebSocket):
//!   Client → Server: subscribe, unsubscribe, force, unforce, getCatalog, getCycleInfo
//!   Server → Client: response, variableUpdate (pushed), catalog, cycleInfo, error
//!
//! The engine thread snapshots ALL monitorable variables every cycle and
//! sends a broadcast notification. Each WebSocket client has a push task
//! that wakes on these notifications, filters by subscription, and sends
//! only the subscribed variables to the client — throttled to 50ms (20 Hz).

use crate::runtime_manager::RuntimeStatus;
use crate::server::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use futures_util::{SinkExt, StreamExt};
use st_monitor::protocol::*;

/// GET /api/v1/monitor/ws — WebSocket upgrade handler.
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    tracing::info!("Monitor WS: upgrade request received");
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Per-client WebSocket session.
async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    tracing::info!("Monitor WS: client connected");

    let (ws_tx, mut ws_rx) = socket.split();
    let ws_tx = Arc::new(Mutex::new(ws_tx));

    // Per-client subscription set
    let subscriptions: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Spawn the push task — sends variable updates to this client
    let push_tx = ws_tx.clone();
    let push_subs = subscriptions.clone();
    let push_state = state.clone();
    let mut cycle_rx = state.runtime_manager.subscribe_cycles();
    tracing::debug!("Monitor WS: push task spawned, subscribed to cycle broadcast");

    let push_task = tokio::spawn(async move {
        let mut last_push = Instant::now();
        let throttle = Duration::from_millis(50);
        let mut push_count: u64 = 0;

        loop {
            // Wait for engine cycle notification
            if cycle_rx.recv().await.is_err() {
                tracing::debug!("Monitor WS: cycle broadcast closed, push task exiting");
                break;
            }

            // Throttle: skip if we pushed recently
            let now = Instant::now();
            if now.duration_since(last_push) < throttle {
                continue;
            }

            let subs = push_subs.lock().await;
            if subs.is_empty() {
                continue;
            }

            // Read the latest variable snapshot from shared state
            let rt_state = push_state.runtime_manager.state();
            let cycle_count = rt_state
                .cycle_stats
                .as_ref()
                .map(|cs| cs.cycle_count)
                .unwrap_or(0);

            let vars: Vec<VariableValue> = rt_state
                .all_variables
                .iter()
                .filter(|v| subs.contains(&v.name))
                .map(|v| VariableValue {
                    name: v.name.clone(),
                    value: v.value.clone(),
                    var_type: v.ty.clone(),
                    forced: v.forced,
                })
                .collect();
            let sub_count = subs.len();
            drop(subs);

            if vars.is_empty() {
                if push_count == 0 {
                    tracing::debug!(
                        "Monitor WS: push skip — {sub_count} subscriptions but 0 matched \
                         ({} all_variables in snapshot)",
                        rt_state.all_variables.len()
                    );
                }
                continue;
            }

            let cs = rt_state.cycle_stats.as_ref();
            let msg = MonitorMessage::VariableUpdate(VariableUpdateData {
                cycle: cycle_count,
                last_cycle_us: cs.map(|s| s.last_cycle_time_us).unwrap_or(0),
                min_cycle_us: cs.map(|s| s.min_cycle_time_us).unwrap_or(0),
                max_cycle_us: cs.map(|s| s.max_cycle_time_us).unwrap_or(0),
                avg_cycle_us: cs.map(|s| s.avg_cycle_time_us).unwrap_or(0),
                target_cycle_us: cs.map(|s| s.target_cycle_us).unwrap_or(0),
                last_period_us: cs.map(|s| s.last_period_us).unwrap_or(0),
                min_period_us: cs.map(|s| s.min_period_us).unwrap_or(0),
                max_period_us: cs.map(|s| s.max_period_us).unwrap_or(0),
                jitter_max_us: cs.map(|s| s.jitter_max_us).unwrap_or(0),
                variables: vars.clone(),
            });

            if let Ok(json) = serde_json::to_string(&msg) {
                let mut tx = push_tx.lock().await;
                if tx.send(Message::Text(json.into())).await.is_err() {
                    tracing::info!("Monitor WS: push send failed, client disconnected");
                    break;
                }
                push_count += 1;
                if push_count == 1 {
                    tracing::info!(
                        "Monitor WS: first push sent — {} vars at cycle {}",
                        vars.len(),
                        cycle_count
                    );
                } else if push_count % 200 == 0 {
                    tracing::debug!(
                        "Monitor WS: push #{push_count} — {} vars at cycle {}",
                        vars.len(),
                        cycle_count
                    );
                }
                last_push = Instant::now();
            }
        }
    });

    // Handle incoming messages from the client
    while let Some(Ok(msg)) = ws_rx.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(reason) => {
                tracing::info!("Monitor WS: client sent Close frame: {reason:?}");
                break;
            }
            Message::Ping(_) => continue,
            Message::Pong(_) => continue,
            other => {
                tracing::debug!("Monitor WS: ignoring non-text frame: {other:?}");
                continue;
            }
        };

        tracing::debug!("Monitor WS: recv ← {text}");

        let request: MonitorRequest = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Monitor WS: invalid JSON from client: {e}");
                let err = MonitorMessage::Error(ErrorData {
                    message: format!("Invalid request: {e}"),
                });
                if let Ok(json) = serde_json::to_string(&err) {
                    let mut tx = ws_tx.lock().await;
                    let _ = tx.send(Message::Text(json.into())).await;
                }
                continue;
            }
        };

        let response = handle_request(request, &state, &subscriptions).await;
        if let Ok(json) = serde_json::to_string(&response) {
            tracing::debug!("Monitor WS: send → {json}");
            let mut tx = ws_tx.lock().await;
            if tx.send(Message::Text(json.into())).await.is_err() {
                tracing::info!("Monitor WS: send failed, client disconnected");
                break;
            }
        }
    }

    tracing::info!("Monitor WS: client session ended, cleaning up");
    push_task.abort();
}

/// Handle a single client request and return a response message.
async fn handle_request(
    request: MonitorRequest,
    state: &Arc<AppState>,
    subscriptions: &Arc<Mutex<HashSet<String>>>,
) -> MonitorMessage {
    match request {
        MonitorRequest::Subscribe(params) => {
            let mut subs = subscriptions.lock().await;
            for var in &params.variables {
                subs.insert(var.clone());
            }
            tracing::info!(
                "Monitor WS: subscribe — added {:?}, total {} subscriptions",
                params.variables,
                subs.len()
            );
            // Log what the snapshot currently has for debugging name mismatches
            let all = state.runtime_manager.all_variables();
            if !all.is_empty() {
                let sample: Vec<&str> = all.iter().take(5).map(|v| v.name.as_str()).collect();
                tracing::debug!(
                    "Monitor WS: snapshot has {} vars, first 5: {:?}",
                    all.len(),
                    sample
                );
            } else {
                tracing::debug!("Monitor WS: snapshot is EMPTY (engine not running?)");
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
            let mut subs = subscriptions.lock().await;
            for var in &params.variables {
                subs.remove(var);
            }
            tracing::info!(
                "Monitor WS: unsubscribe — removed {:?}, {} remaining",
                params.variables,
                subs.len()
            );
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: None,
            })
        }
        MonitorRequest::GetCatalog => {
            let catalog = state.runtime_manager.variable_catalog();
            tracing::info!(
                "Monitor WS: getCatalog — returning {} variables",
                catalog.len()
            );
            if catalog.is_empty() {
                tracing::warn!(
                    "Monitor WS: catalog is empty! Status={:?}",
                    state.runtime_manager.state().status
                );
            } else {
                let sample: Vec<&str> = catalog.iter().take(5).map(|c| c.name.as_str()).collect();
                tracing::debug!("Monitor WS: catalog sample: {sample:?}");
            }
            MonitorMessage::Catalog(CatalogData {
                variables: catalog
                    .into_iter()
                    .map(|c| CatalogEntry {
                        name: c.name,
                        var_type: c.ty,
                    })
                    .collect(),
            })
        }
        MonitorRequest::GetCycleInfo => {
            let rt_state = state.runtime_manager.state();
            let cs = rt_state.cycle_stats.unwrap_or_default();
            tracing::debug!(
                "Monitor WS: getCycleInfo — cycle={}, last={}µs",
                cs.cycle_count,
                cs.last_cycle_time_us
            );
            MonitorMessage::CycleInfo(CycleInfoData {
                cycle_count: cs.cycle_count,
                last_cycle_us: cs.last_cycle_time_us,
                min_cycle_us: cs.min_cycle_time_us,
                max_cycle_us: cs.max_cycle_time_us,
                avg_cycle_us: cs.avg_cycle_time_us,
                target_cycle_us: cs.target_cycle_us,
                last_period_us: cs.last_period_us,
                min_period_us: cs.min_period_us,
                max_period_us: cs.max_period_us,
                jitter_max_us: cs.jitter_max_us,
            })
        }
        MonitorRequest::Read(params) => {
            let all = state.runtime_manager.all_variables();
            tracing::debug!(
                "Monitor WS: read {:?} — snapshot has {} vars",
                params.variables,
                all.len()
            );
            let vars: Vec<VariableValue> = params
                .variables
                .iter()
                .filter_map(|name| {
                    all.iter()
                        .find(|v| v.name.eq_ignore_ascii_case(name))
                        .map(|v| VariableValue {
                            name: v.name.clone(),
                            value: v.value.clone(),
                            var_type: v.ty.clone(),
                            forced: v.forced,
                        })
                })
                .collect();
            tracing::debug!("Monitor WS: read matched {} of {}", vars.len(), params.variables.len());
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: Some(serde_json::to_value(vars).unwrap()),
            })
        }
        MonitorRequest::Force(params) => {
            tracing::info!(
                "Monitor WS: force {} = {:?}",
                params.variable,
                params.value
            );
            let current_status = state.runtime_manager.state().status;
            if current_status != RuntimeStatus::Running
                && current_status != RuntimeStatus::DebugPaused
            {
                tracing::warn!("Monitor WS: force rejected — status is {current_status:?}");
                return MonitorMessage::Error(ErrorData {
                    message: "Runtime is not running".to_string(),
                });
            }
            let value_str = match &params.value {
                serde_json::Value::Bool(b) => {
                    if *b { "true" } else { "false" }.to_string()
                }
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => {
                    tracing::warn!("Monitor WS: force rejected — invalid value type");
                    return MonitorMessage::Error(ErrorData {
                        message: "Invalid value type".to_string(),
                    });
                }
            };
            match state
                .runtime_manager
                .force_variable(params.variable.clone(), value_str)
                .await
            {
                Ok(result) => {
                    tracing::info!("Monitor WS: force OK — {result}");
                    MonitorMessage::Response(ResponseData {
                        id: None,
                        success: true,
                        data: Some(serde_json::json!({
                            "variable": params.variable,
                            "result": result,
                        })),
                    })
                }
                Err(e) => {
                    tracing::warn!("Monitor WS: force failed — {e}");
                    MonitorMessage::Error(ErrorData {
                        message: e.to_string(),
                    })
                }
            }
        }
        MonitorRequest::Unforce(params) => {
            tracing::info!("Monitor WS: unforce {}", params.variable);
            match state
                .runtime_manager
                .unforce_variable(params.variable.clone())
                .await
            {
                Ok(()) => {
                    tracing::info!("Monitor WS: unforce OK");
                    MonitorMessage::Response(ResponseData {
                        id: None,
                        success: true,
                        data: Some(serde_json::json!({
                            "variable": params.variable,
                            "unforced": true,
                        })),
                    })
                }
                Err(e) => {
                    tracing::warn!("Monitor WS: unforce failed — {e}");
                    MonitorMessage::Error(ErrorData {
                        message: e.to_string(),
                    })
                }
            }
        }
        MonitorRequest::ResetStats => {
            tracing::info!("Monitor WS: resetStats");
            let _ = state.runtime_manager.reset_stats().await;
            MonitorMessage::Response(ResponseData {
                id: None,
                success: true,
                data: None,
            })
        }
        MonitorRequest::Write(_) | MonitorRequest::OnlineChange(_) => {
            tracing::warn!("Monitor WS: unsupported request type");
            MonitorMessage::Error(ErrorData {
                message: "Not implemented".to_string(),
            })
        }
    }
}
