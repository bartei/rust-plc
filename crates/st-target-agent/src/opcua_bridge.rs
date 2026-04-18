//! Bridge between the RuntimeManager and the OPC-UA server's PlcDataProvider trait.
//!
//! Wraps `Arc<AppState>` to implement `PlcDataProvider` by delegating to
//! `RuntimeManager`'s existing public methods. No new code touches the
//! engine or scan cycle — we only read from `RuntimeState` and send
//! commands through the existing `RuntimeCommand` channel.

use crate::server::AppState;
use async_trait::async_trait;
use std::sync::Arc;

/// Adapter that implements [`st_opcua_server::PlcDataProvider`] by wrapping
/// the agent's `AppState` (which owns the `RuntimeManager`).
pub struct AgentDataProvider {
    state: Arc<AppState>,
}

impl AgentDataProvider {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl st_opcua_server::PlcDataProvider for AgentDataProvider {
    fn variable_catalog(&self) -> Vec<st_opcua_server::CatalogEntry> {
        self.state
            .runtime_manager
            .variable_catalog()
            .into_iter()
            .map(|e| st_opcua_server::CatalogEntry {
                name: e.name,
                iec_type: e.ty,
            })
            .collect()
    }

    fn all_variables(&self) -> Vec<st_opcua_server::VariableSnapshot> {
        self.state
            .runtime_manager
            .all_variables()
            .into_iter()
            .map(|v| st_opcua_server::VariableSnapshot {
                name: v.name,
                value: v.value,
                iec_type: v.ty,
                forced: v.forced,
            })
            .collect()
    }

    fn runtime_status(&self) -> String {
        let state = self.state.runtime_manager.state();
        format!("{:?}", state.status)
    }

    fn cycle_stats(&self) -> Option<st_opcua_server::CycleStats> {
        let state = self.state.runtime_manager.state();
        state.cycle_stats.map(|cs| st_opcua_server::CycleStats {
            cycle_count: cs.cycle_count,
            last_cycle_time_us: cs.last_cycle_time_us,
            min_cycle_time_us: cs.min_cycle_time_us,
            max_cycle_time_us: cs.max_cycle_time_us,
            avg_cycle_time_us: cs.avg_cycle_time_us,
        })
    }

    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String> {
        self.state
            .runtime_manager
            .force_variable(name.to_string(), value.to_string())
            .await
            .map_err(|e| e.to_string())
    }

    async fn unforce_variable(&self, name: &str) -> Result<(), String> {
        self.state
            .runtime_manager
            .unforce_variable(name.to_string())
            .await
            .map_err(|e| e.to_string())
    }
}
