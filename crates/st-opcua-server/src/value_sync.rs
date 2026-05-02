//! Background task that syncs PLC variable values to OPC-UA nodes.
//!
//! Polls the [`PlcDataProvider`] at the configured sampling interval,
//! parses string values to OPC-UA Variants, and updates node DataValues
//! in the address space. Subscription notifications are sent automatically
//! by the `SimpleNodeManager` when values change.

use crate::address_space::{self as addr, AddressSpaceLayout, PLC_NAMESPACE};
use crate::type_map::parse_value_to_variant;
use crate::write_handler;
use crate::CatalogEntry;
use crate::PlcDataProvider;
use opcua_server::node_manager::memory::SimpleNodeManager;
use opcua_server::ServerHandle;
use opcua_types::{DataValue, DateTime, NodeId, StatusCode, Variant};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Run the value sync loop. Polls the provider at the configured interval
/// and updates OPC-UA node values. Also detects catalog changes and
/// rebuilds the address space when necessary.
pub async fn run_value_sync(
    handle: ServerHandle,
    provider: Arc<dyn PlcDataProvider>,
    sampling_interval: Duration,
) {
    // Initialize prev_catalog from the current state so the first sync
    // doesn't trigger a spurious rebuild (nodes were already built by
    // run_opcua_server before this task started).
    let mut prev_catalog: Vec<CatalogEntry> = provider.variable_catalog();
    // Track all NodeIds we created so we can clean them up on rebuild.
    let mut tracked_nodes: Vec<NodeId> = collect_layout_node_ids(
        &addr::build_layout(&prev_catalog),
    );
    let mut sync_count: u64 = 0;
    // Dedupe warn-spam from the per-cycle value-sync writes: log each
    // (NodeId, StatusCode) pair at most once per catalog generation, so a
    // permanently-broken node produces a single warning instead of one per
    // sync interval. Cleared whenever the catalog is rebuilt.
    let mut warned: HashSet<(NodeId, StatusCode)> = HashSet::new();

    tracing::info!(
        "OPC-UA: value sync task starting (interval={}ms)",
        sampling_interval.as_millis()
    );

    loop {
        tokio::time::sleep(sampling_interval).await;

        // Check for catalog changes (program start/stop/online change)
        let catalog = provider.variable_catalog();
        if catalog_changed(&prev_catalog, &catalog) {
            tracked_nodes = rebuild_address_space(&handle, &catalog, &tracked_nodes, &provider);
            prev_catalog = catalog.clone();
            warned.clear();
        }

        // Sync values from PLC to OPC-UA nodes
        let variables = provider.all_variables();
        update_node_values(&handle, &variables, &mut warned);

        // Update status and cycle stats nodes
        update_status_nodes(&handle, &provider);

        sync_count += 1;
        if sync_count == 1 {
            tracing::info!(
                "OPC-UA: first value sync — {} variables",
                variables.len()
            );
        } else if sync_count % 10_000 == 0 {
            tracing::info!(
                "OPC-UA: value sync #{sync_count} — {} variables",
                variables.len()
            );
        }
    }
}

fn catalog_changed(prev: &[CatalogEntry], current: &[CatalogEntry]) -> bool {
    if prev.len() != current.len() {
        return true;
    }
    prev.iter()
        .zip(current.iter())
        .any(|(a, b)| a.name != b.name || a.iec_type != b.iec_type)
}

/// Collect all NodeIds from a layout (folders + variables + status nodes + root).
fn collect_layout_node_ids(layout: &AddressSpaceLayout) -> Vec<NodeId> {
    let mut ids = Vec::new();
    // Variables first (leaf nodes), then folders (parent nodes), then root.
    // Deletion order: leaves before parents to avoid orphan references.
    for var in &layout.variables {
        ids.push(var.node_id.clone());
    }
    for folder in &layout.folders {
        ids.push(folder.node_id.clone());
    }
    // Status nodes
    ids.push(addr::status_node_id());
    ids.push(addr::cycle_count_node_id());
    ids.push(addr::cycle_time_node_id());
    // Root folder last
    ids.push(layout.root_folder.clone());
    ids
}

/// Rebuild the address space: delete old nodes, create new ones, register
/// write callbacks. Returns the new set of tracked NodeIds.
fn rebuild_address_space(
    handle: &ServerHandle,
    catalog: &[CatalogEntry],
    old_nodes: &[NodeId],
    provider: &Arc<dyn PlcDataProvider>,
) -> Vec<NodeId> {
    let Some(nm) = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
    else {
        tracing::warn!("OPC-UA: cannot find SimpleNodeManager for address space rebuild");
        return Vec::new();
    };

    let layout = addr::build_layout(catalog);

    {
        let mut address_space = nm.address_space().write();

        // Delete old nodes (leaves first, root last)
        for node_id in old_nodes {
            address_space.delete(node_id, true);
        }

        // Build new nodes
        build_nodes_from_layout(&mut address_space, &layout);
    }

    // Register write callbacks for new variables
    let var_names: Vec<String> = catalog.iter().map(|e| e.name.clone()).collect();
    write_handler::register_write_callbacks(handle, Arc::clone(provider), &var_names);

    let new_tracked = collect_layout_node_ids(&layout);

    if catalog.is_empty() {
        tracing::info!("OPC-UA: address space cleared — runtime stopped, 0 variables");
    } else {
        tracing::info!(
            "OPC-UA: catalog changed — rebuilt address space: {} variable nodes, {} folder nodes",
            layout.variables.len(),
            layout.folders.len()
        );
    }

    new_tracked
}

/// Build OPC-UA nodes from the address space layout.
pub fn build_nodes_from_layout(
    address_space: &mut opcua_server::address_space::AddressSpace,
    layout: &AddressSpaceLayout,
) {
    use opcua_server::address_space::VariableBuilder;
    use opcua_types::ObjectId;

    // Create the PLCRuntime root folder under Objects
    address_space.add_folder(
        &layout.root_folder,
        "PLCRuntime",
        "PLCRuntime",
        &ObjectId::ObjectsFolder.into(),
    );

    // Create status nodes
    VariableBuilder::new(&addr::status_node_id(), "_status", "Status")
        .data_type(opcua_types::DataTypeId::String)
        .value(Variant::String("Idle".to_string().into()))
        .organized_by(layout.root_folder.clone())
        .insert(address_space);

    VariableBuilder::new(&addr::cycle_count_node_id(), "_cycle_count", "Cycle Count")
        .data_type(opcua_types::DataTypeId::UInt64)
        .value(Variant::UInt64(0))
        .organized_by(layout.root_folder.clone())
        .insert(address_space);

    VariableBuilder::new(
        &addr::cycle_time_node_id(),
        "_cycle_time_us",
        "Cycle Time (us)",
    )
    .data_type(opcua_types::DataTypeId::UInt64)
    .value(Variant::UInt64(0))
    .organized_by(layout.root_folder.clone())
    .insert(address_space);

    // Create folders
    for folder in &layout.folders {
        address_space.add_folder(&folder.node_id, &folder.name, &folder.name, &folder.parent);
    }

    // Create variable nodes
    for var in &layout.variables {
        let initial_value = default_value_for_type(&var.iec_type);
        VariableBuilder::new(&var.node_id, &var.browse_name, &var.display_name)
            .data_type(var.data_type)
            .value(initial_value)
            .writable()
            .organized_by(var.parent_folder.clone())
            .insert(address_space);
    }
}

fn default_value_for_type(iec_type: &str) -> Variant {
    match iec_type.to_uppercase().as_str() {
        "BOOL" => Variant::Boolean(false),
        "SINT" => Variant::SByte(0),
        "INT" => Variant::Int16(0),
        "DINT" => Variant::Int32(0),
        "LINT" => Variant::Int64(0),
        "USINT" | "BYTE" => Variant::Byte(0),
        "UINT" | "WORD" => Variant::UInt16(0),
        "UDINT" | "DWORD" => Variant::UInt32(0),
        "ULINT" | "LWORD" => Variant::UInt64(0),
        "REAL" => Variant::Float(0.0),
        "LREAL" => Variant::Double(0.0),
        "STRING" => Variant::String("".to_string().into()),
        "TIME" => Variant::Int64(0),
        _ => Variant::String("".to_string().into()),
    }
}

fn update_node_values(
    handle: &ServerHandle,
    variables: &[crate::VariableSnapshot],
    warned: &mut HashSet<(NodeId, StatusCode)>,
) {
    let Some(nm) = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
    else {
        return;
    };

    let now = DateTime::now();
    let subscriptions = handle.subscriptions();

    // Build owned NodeIds and DataValues
    let owned_node_ids: Vec<NodeId> = variables
        .iter()
        .map(|var| NodeId::new(PLC_NAMESPACE, var.name.as_str()))
        .collect();

    let data_values: Vec<DataValue> = variables
        .iter()
        .map(|var| {
            let variant = parse_value_to_variant(&var.value, &var.iec_type);
            DataValue {
                value: Some(variant),
                status: Some(StatusCode::Good),
                source_timestamp: Some(now),
                server_timestamp: Some(now),
                ..Default::default()
            }
        })
        .collect();

    // Fast path: batched write. If it fails, the SimpleNodeManager only
    // returns the first failing StatusCode — we have no idea which node
    // caused it. Fall back to per-node writes to identify the culprit.
    let items: Vec<(&NodeId, Option<&opcua_types::NumericRange>, DataValue)> = owned_node_ids
        .iter()
        .zip(data_values.iter().cloned())
        .map(|(id, dv)| (id, None, dv))
        .collect();

    if nm.set_values(subscriptions, items.into_iter()).is_ok() {
        return;
    }

    // Slow diagnostic path: write one node at a time so the warn message
    // can name the offending NodeId. Each (NodeId, StatusCode) pair is
    // logged at most once until the next catalog rebuild.
    for ((id, dv), var) in owned_node_ids
        .iter()
        .zip(data_values.into_iter())
        .zip(variables.iter())
    {
        let item = std::iter::once((
            id,
            None::<&opcua_types::NumericRange>,
            dv,
        ));
        if let Err(e) = nm.set_values(subscriptions, item) {
            // BadNodeIdUnknown is expected during catalog transitions and
            // doesn't deserve a warning even on the slow path.
            if e == StatusCode::BadNodeIdUnknown {
                continue;
            }
            if warned.insert((id.clone(), e)) {
                tracing::warn!(
                    "OPC-UA: value sync error on node {} (iec_type={}) — {}",
                    id,
                    var.iec_type,
                    e,
                );
            }
        }
    }
}

fn update_status_nodes(handle: &ServerHandle, provider: &Arc<dyn PlcDataProvider>) {
    let Some(nm) = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
    else {
        return;
    };

    let status = provider.runtime_status();
    let cycle_stats = provider.cycle_stats();

    let subscriptions = handle.subscriptions();
    let now = DateTime::now();

    let mut items: Vec<(&NodeId, Option<&opcua_types::NumericRange>, DataValue)> = Vec::new();

    let status_node = addr::status_node_id();
    let status_dv = DataValue {
        value: Some(Variant::String(status.into())),
        status: Some(StatusCode::Good),
        source_timestamp: Some(now),
        server_timestamp: Some(now),
        ..Default::default()
    };
    items.push((&status_node, None, status_dv));

    let cycle_count_node = addr::cycle_count_node_id();
    let cycle_time_node = addr::cycle_time_node_id();

    let cycle_count_dv;
    let cycle_time_dv;

    if let Some(stats) = cycle_stats {
        cycle_count_dv = DataValue {
            value: Some(Variant::UInt64(stats.cycle_count)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(now),
            server_timestamp: Some(now),
            ..Default::default()
        };
        cycle_time_dv = DataValue {
            value: Some(Variant::UInt64(stats.last_cycle_time_us)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(now),
            server_timestamp: Some(now),
            ..Default::default()
        };
    } else {
        cycle_count_dv = DataValue {
            value: Some(Variant::UInt64(0)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(now),
            server_timestamp: Some(now),
            ..Default::default()
        };
        cycle_time_dv = DataValue {
            value: Some(Variant::UInt64(0)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(now),
            server_timestamp: Some(now),
            ..Default::default()
        };
    }

    items.push((&cycle_count_node, None, cycle_count_dv));
    items.push((&cycle_time_node, None, cycle_time_dv));

    let _ = nm.set_values(subscriptions, items.into_iter());
}
