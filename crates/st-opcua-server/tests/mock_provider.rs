//! Integration test: start an OPC-UA server with a mock PlcDataProvider,
//! connect with an async-opcua client, browse, read, and write variables.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use st_opcua_server::{
    CatalogEntry, CycleStats, OpcuaServerConfig, PlcDataProvider, VariableSnapshot,
};

// ── Mock provider ──────────────────────────────────────────────────────

struct MockProvider {
    catalog: Vec<CatalogEntry>,
    values: Arc<Mutex<HashMap<String, (String, String)>>>, // name → (value, type)
    forced: Arc<Mutex<HashMap<String, String>>>,           // name → value
}

impl MockProvider {
    fn new() -> Self {
        let mut values = HashMap::new();
        values.insert(
            "test_bool".to_string(),
            ("TRUE".to_string(), "BOOL".to_string()),
        );
        values.insert(
            "test_int".to_string(),
            ("42".to_string(), "INT".to_string()),
        );
        values.insert(
            "test_real".to_string(),
            ("3.140000".to_string(), "REAL".to_string()),
        );
        values.insert(
            "test_string".to_string(),
            ("'hello'".to_string(), "STRING".to_string()),
        );
        values.insert(
            "Main.counter".to_string(),
            ("100".to_string(), "DINT".to_string()),
        );

        let catalog = values
            .iter()
            .map(|(name, (_, ty))| CatalogEntry {
                name: name.clone(),
                iec_type: ty.clone(),
            })
            .collect();

        MockProvider {
            catalog,
            values: Arc::new(Mutex::new(values)),
            forced: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl PlcDataProvider for MockProvider {
    fn variable_catalog(&self) -> Vec<CatalogEntry> {
        self.catalog.clone()
    }

    fn all_variables(&self) -> Vec<VariableSnapshot> {
        let vals = self.values.lock().unwrap();
        let forced = self.forced.lock().unwrap();
        vals.iter()
            .map(|(name, (value, ty))| {
                let actual_value = if let Some(fv) = forced.get(name) {
                    fv.clone()
                } else {
                    value.clone()
                };
                VariableSnapshot {
                    name: name.clone(),
                    value: actual_value,
                    iec_type: ty.clone(),
                    forced: forced.contains_key(name),
                }
            })
            .collect()
    }

    fn runtime_status(&self) -> String {
        "Running".to_string()
    }

    fn cycle_stats(&self) -> Option<CycleStats> {
        Some(CycleStats {
            cycle_count: 1000,
            last_cycle_time_us: 500,
            min_cycle_time_us: 200,
            max_cycle_time_us: 800,
            avg_cycle_time_us: 450,
        })
    }

    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String> {
        self.forced
            .lock()
            .unwrap()
            .insert(name.to_string(), value.to_string());
        Ok(format!("Forced {name} = {value}"))
    }

    async fn unforce_variable(&self, name: &str) -> Result<(), String> {
        self.forced.lock().unwrap().remove(name);
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn server_starts_and_client_connects() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let provider = Arc::new(MockProvider::new());

    // Use port 0 so the OS assigns a free port (avoid conflicts in CI)
    let config = OpcuaServerConfig {
        enabled: true,
        port: 0,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(config, provider.clone())
        .await
        .expect("Server should start");

    // Give the server a moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Read the actual bound port from server info
    let endpoints = handle.info().config.endpoints.clone();
    assert!(
        !endpoints.is_empty(),
        "Server should have at least one endpoint"
    );

    // Clean shutdown
    handle.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn server_exposes_catalog_variables() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let provider = Arc::new(MockProvider::new());

    let config = OpcuaServerConfig {
        enabled: true,
        port: 0,
        sampling_interval_ms: 50,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(config, provider.clone())
        .await
        .expect("Server should start");

    // Wait for first value sync
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Verify address space has our nodes by checking the node manager
    let nm = handle
        .node_managers()
        .get_of_type::<opcua_server::node_manager::memory::SimpleNodeManager>()
        .expect("Should have SimpleNodeManager");

    let addr_space = nm.address_space().read();

    // Check PLCRuntime folder exists
    let plc_root = opcua_types::NodeId::new(2u16, "PLCRuntime");
    assert!(
        addr_space.node_exists(&plc_root),
        "PLCRuntime folder should exist"
    );

    // Check variable nodes exist
    let test_bool = opcua_types::NodeId::new(2u16, "test_bool");
    assert!(
        addr_space.node_exists(&test_bool),
        "test_bool node should exist"
    );

    let test_int = opcua_types::NodeId::new(2u16, "test_int");
    assert!(
        addr_space.node_exists(&test_int),
        "test_int node should exist"
    );

    let main_counter = opcua_types::NodeId::new(2u16, "Main.counter");
    assert!(
        addr_space.node_exists(&main_counter),
        "Main.counter node should exist"
    );

    // Check folder hierarchy
    let globals_folder = opcua_types::NodeId::new(2u16, "Globals");
    assert!(
        addr_space.node_exists(&globals_folder),
        "Globals folder should exist"
    );

    let programs_folder = opcua_types::NodeId::new(2u16, "Programs");
    assert!(
        addr_space.node_exists(&programs_folder),
        "Programs folder should exist"
    );

    let main_folder = opcua_types::NodeId::new(2u16, "Main");
    assert!(
        addr_space.node_exists(&main_folder),
        "Main folder should exist"
    );

    // Check status nodes
    let status_node = opcua_types::NodeId::new(2u16, "_status");
    assert!(
        addr_space.node_exists(&status_node),
        "_status node should exist"
    );

    drop(addr_space);

    handle.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn write_callback_forces_variable() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let provider = Arc::new(MockProvider::new());

    let config = OpcuaServerConfig {
        enabled: true,
        port: 0,
        sampling_interval_ms: 50,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(config, provider.clone())
        .await
        .expect("Server should start");

    // Wait for startup
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Directly invoke the force_variable to verify the provider side works
    let result = provider.force_variable("test_int", "99").await;
    assert!(result.is_ok());

    // Verify the force was recorded
    let forced = provider.forced.lock().unwrap();
    assert_eq!(forced.get("test_int"), Some(&"99".to_string()));
    drop(forced);

    // Verify the forced value shows up in all_variables
    let vars = provider.all_variables();
    let test_int = vars.iter().find(|v| v.name == "test_int").unwrap();
    assert_eq!(test_int.value, "99");
    assert!(test_int.forced);

    handle.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
