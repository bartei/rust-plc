//! OPC-UA server for the ST PLC runtime.
//!
//! Exposes PLC variables to HMI and SCADA clients via the OPC-UA protocol.
//! This is an **export layer** — it reads PLC state and allows external clients
//! to browse, subscribe, and write PLC variables.
//!
//! # Architecture
//!
//! The server is decoupled from the PLC engine via the [`PlcDataProvider`] trait.
//! The host application (e.g., `st-target-agent`) implements this trait by wrapping
//! its `RuntimeManager`, then passes it to [`run_opcua_server`].
//!
//! The server runs as tokio tasks — no dedicated threads. It reads variable
//! snapshots from the provider at a configurable polling interval and updates
//! the OPC-UA address space. OPC-UA subscriptions automatically push changes
//! to connected clients.

pub mod config;
pub mod type_map;
pub mod address_space;
pub mod value_sync;
pub mod write_handler;

pub use config::OpcuaServerConfig;

use async_trait::async_trait;

// ── Data types exchanged with the host application ─────────────────────

/// A variable in the PLC catalog (schema only, no value).
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    /// Variable name (e.g., `"io_rack_DI_0"`, `"Main.counter"`).
    pub name: String,
    /// IEC 61131-3 type string (e.g., `"BOOL"`, `"INT"`, `"REAL"`).
    pub iec_type: String,
}

/// A snapshot of a single variable's current value.
#[derive(Debug, Clone)]
pub struct VariableSnapshot {
    /// Variable name.
    pub name: String,
    /// Current value as a display string (same format as `debug::format_value`).
    pub value: String,
    /// IEC 61131-3 type string.
    pub iec_type: String,
    /// Whether this variable is currently forced.
    pub forced: bool,
}

/// Cycle statistics snapshot.
#[derive(Debug, Clone, Default)]
pub struct CycleStats {
    pub cycle_count: u64,
    pub last_cycle_time_us: u64,
    pub min_cycle_time_us: u64,
    pub max_cycle_time_us: u64,
    pub avg_cycle_time_us: u64,
}

// ── Provider trait ─────────────────────────────────────────────────────

/// Abstraction over the PLC runtime's variable interface.
///
/// The host application implements this trait to bridge between the OPC-UA
/// server and the PLC engine. The OPC-UA server only depends on this trait,
/// not on any engine internals.
#[async_trait]
pub trait PlcDataProvider: Send + Sync + 'static {
    /// Get the variable catalog (names + IEC types). Empty if no program running.
    fn variable_catalog(&self) -> Vec<CatalogEntry>;

    /// Get current values of all variables.
    fn all_variables(&self) -> Vec<VariableSnapshot>;

    /// Get the current runtime status as a string (`"Running"`, `"Idle"`, `"Error"`).
    fn runtime_status(&self) -> String;

    /// Get cycle statistics. `None` if no program is running.
    fn cycle_stats(&self) -> Option<CycleStats>;

    /// Force a variable to a value. Returns a description on success.
    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String>;

    /// Remove a force override from a variable.
    async fn unforce_variable(&self, name: &str) -> Result<(), String>;
}

// ── Public API ─────────────────────────────────────────────────────────

/// Start the OPC-UA server. This spawns background tokio tasks and returns
/// the [`opcua_server::ServerHandle`] which can be used to shut down the
/// server. The server runs until cancelled via the handle or the tokio
/// runtime shuts down.
///
/// # Errors
///
/// Returns an error if the server cannot be built or started.
pub async fn run_opcua_server(
    config: OpcuaServerConfig,
    provider: std::sync::Arc<dyn PlcDataProvider>,
) -> Result<opcua_server::ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    use opcua_server::node_manager::memory::simple_node_manager;
    use opcua_server::diagnostics::NamespaceMetadata;
    use opcua_server::ServerBuilder;

    let endpoint_url = config.endpoint_url();
    tracing::info!("OPC-UA server starting on {endpoint_url}");
    tracing::info!(
        "OPC-UA server: security policy={}, anonymous={}",
        config.security_policy,
        config.anonymous_access,
    );

    // Build the OPC-UA server
    let ns = NamespaceMetadata {
        namespace_uri: "urn:st-plc:opcua:plc-variables".to_string(),
        ..Default::default()
    };

    // Resolve the PKI directory for certificate storage.
    // On the target agent this is /var/lib/st-plc/pki; for local dev we use ./pki.
    let pki_dir = config
        .pki_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./pki"));

    let mut builder = ServerBuilder::new_anonymous(&config.application_name)
        .application_uri("urn:st-plc:opcua:server")
        .host(&config.bind)
        .port(config.port)
        .pki_dir(&pki_dir)
        .create_sample_keypair(true)
        .certificate_path("own/cert.der")
        .private_key_path("private/private.pem")
        .with_node_manager(simple_node_manager(ns, "PLCVariables"));

    // Configure subscription polling to match our sampling interval
    builder = builder.subscription_poll_interval_ms(config.sampling_interval_ms);

    tracing::info!("OPC-UA server: PKI directory = {}", pki_dir.display());

    let (server, handle) = builder
        .build()
        .map_err(|e| format!("OPC-UA server build failed: {e}"))?;

    // Build initial address space from current catalog
    let catalog = provider.variable_catalog();
    {
        let nm = handle
            .node_managers()
            .get_of_type::<opcua_server::node_manager::memory::SimpleNodeManager>()
            .ok_or("Cannot find SimpleNodeManager")?;

        let mut addr_space = nm.address_space().write();
        let layout = address_space::build_layout(&catalog);
        value_sync::build_nodes_from_layout(&mut addr_space, &layout);

        tracing::info!(
            "OPC-UA: building address space from {} variables",
            catalog.len()
        );
    }

    // Register write callbacks for current variables
    let var_names: Vec<String> = catalog.iter().map(|e| e.name.clone()).collect();
    write_handler::register_write_callbacks(&handle, provider.clone(), &var_names);

    // Spawn the value sync background task
    let sync_handle = handle.clone();
    let sync_provider = provider.clone();
    let sampling_interval =
        std::time::Duration::from_millis(config.sampling_interval_ms);
    tokio::spawn(async move {
        value_sync::run_value_sync(sync_handle, sync_provider, sampling_interval).await;
    });

    // Spawn the OPC-UA server itself
    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("OPC-UA: server error — {e}");
        }
    });

    tracing::info!("OPC-UA server ready, waiting for connections");
    Ok(handle)
}
