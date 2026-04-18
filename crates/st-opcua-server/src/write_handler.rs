//! OPC-UA Write service handler.
//!
//! Registers write callbacks on PLC variable nodes so that when an OPC-UA
//! client writes to a variable, the value is forwarded to the PLC engine
//! via `PlcDataProvider::force_variable()`.

use crate::address_space::PLC_NAMESPACE;
use crate::type_map::variant_to_value_string;
use crate::PlcDataProvider;
use opcua_server::node_manager::memory::SimpleNodeManager;
use opcua_server::ServerHandle;
use opcua_types::{DataValue, NodeId, NumericRange, StatusCode};
use std::sync::Arc;

/// Register write callbacks for all PLC variable nodes in the address space.
///
/// Each callback extracts the variable name from the NodeId, converts the
/// OPC-UA Variant to a PLC value string, and calls `force_variable()`.
///
/// Because the SimpleNodeManager write callbacks are synchronous (they return
/// a `StatusCode` immediately), we spawn a tokio task for the async
/// `force_variable()` call and return `Good` optimistically. The force will
/// be applied on the next scan cycle regardless.
pub fn register_write_callbacks(
    handle: &ServerHandle,
    provider: Arc<dyn PlcDataProvider>,
    variable_names: &[String],
) {
    let Some(nm) = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
    else {
        tracing::warn!("OPC-UA: cannot find SimpleNodeManager for write callback registration");
        return;
    };

    for name in variable_names {
        let node_id = NodeId::new(PLC_NAMESPACE, name.as_str());
        let var_name = name.clone();
        let provider = Arc::clone(&provider);

        nm.inner().add_write_callback(
            node_id,
            move |data_value: DataValue, _range: &NumericRange| {
                let Some(variant) = &data_value.value else {
                    tracing::warn!(
                        "OPC-UA: write rejected — ns=2;s={var_name} — no value in DataValue"
                    );
                    return StatusCode::BadTypeMismatch;
                };

                let value_str = variant_to_value_string(variant);
                let var_name_clone = var_name.clone();
                let provider_clone = Arc::clone(&provider);

                tracing::info!(
                    "OPC-UA: write request — ns=2;s={var_name} = {value_str} ({:?})",
                    variant
                );

                // Spawn async force_variable — the callback is sync, so we fire-and-forget.
                // The PLC engine applies the force on the next scan cycle.
                tokio::spawn(async move {
                    match provider_clone
                        .force_variable(&var_name_clone, &value_str)
                        .await
                    {
                        Ok(desc) => {
                            tracing::info!("OPC-UA: write applied — {desc}");
                        }
                        Err(e) => {
                            tracing::warn!(
                                "OPC-UA: write failed — ns=2;s={var_name_clone} — {e}"
                            );
                        }
                    }
                });

                StatusCode::Good
            },
        );
    }

    tracing::info!(
        "OPC-UA: registered write callbacks for {} variables",
        variable_names.len()
    );
}
