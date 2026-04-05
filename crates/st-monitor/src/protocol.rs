//! Monitor protocol: JSON-RPC messages over WebSocket.

use serde::{Deserialize, Serialize};

/// Client → Server request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum MonitorRequest {
    /// Subscribe to variable value updates.
    #[serde(rename = "subscribe")]
    Subscribe(SubscribeParams),
    /// Unsubscribe from variable updates.
    #[serde(rename = "unsubscribe")]
    Unsubscribe(UnsubscribeParams),
    /// Read current values of specific variables.
    #[serde(rename = "read")]
    Read(ReadParams),
    /// Write a value to a variable.
    #[serde(rename = "write")]
    Write(WriteParams),
    /// Force a variable to a specific value (overrides runtime).
    #[serde(rename = "force")]
    Force(ForceParams),
    /// Release a forced variable.
    #[serde(rename = "unforce")]
    Unforce(UnforceParams),
    /// Get scan cycle statistics.
    #[serde(rename = "getCycleInfo")]
    GetCycleInfo,
    /// Trigger an online change with new source code.
    #[serde(rename = "onlineChange")]
    OnlineChange(OnlineChangeParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeParams {
    /// Variable names to subscribe to.
    pub variables: Vec<String>,
    /// Update interval in milliseconds (0 = every cycle).
    #[serde(default)]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeParams {
    pub variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadParams {
    pub variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteParams {
    pub variable: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForceParams {
    pub variable: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnforceParams {
    pub variable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineChangeParams {
    pub source: String,
}

/// Server → Client response/event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MonitorMessage {
    /// Response to a request.
    #[serde(rename = "response")]
    Response(ResponseData),
    /// Pushed variable value update.
    #[serde(rename = "variableUpdate")]
    VariableUpdate(VariableUpdateData),
    /// Scan cycle info.
    #[serde(rename = "cycleInfo")]
    CycleInfo(CycleInfoData),
    /// Error.
    #[serde(rename = "error")]
    Error(ErrorData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseData {
    pub id: Option<u64>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableUpdateData {
    pub cycle: u64,
    pub variables: Vec<VariableValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableValue {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub var_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleInfoData {
    pub cycle_count: u64,
    pub last_cycle_us: u64,
    pub min_cycle_us: u64,
    pub max_cycle_us: u64,
    pub avg_cycle_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorData {
    pub message: String,
}

/// Forced variable entry.
#[derive(Debug, Clone)]
pub struct ForcedVariable {
    pub name: String,
    pub value: st_ir::Value,
}
