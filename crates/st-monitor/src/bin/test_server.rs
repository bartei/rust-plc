//! Minimal monitor server for Playwright end-to-end tests.
//!
//! Starts a real WebSocket monitor server with pre-populated test data
//! (FB instances, arrays, scalars). Prints the port to stdout so the
//! test harness can connect.
//!
//! Usage: cargo run -p st-monitor --bin monitor-test-server

use st_monitor::protocol::*;
use st_monitor::server::{MonitorHandle, run_monitor_server};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let handle = MonitorHandle::new();

    // Populate catalog (schema for autocomplete)
    handle.set_catalog(vec![
        CatalogEntry { name: "Main.filler".into(), var_type: "FillController".into() },
        CatalogEntry { name: "Main.filler.start".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.target_fill".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.filler.fill_done".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.fill_count".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.filler.filling".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.counter".into(), var_type: "CTU".into() },
        CatalogEntry { name: "Main.filler.counter.CU".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.counter.RESET".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.counter.PV".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.filler.counter.Q".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.counter.CV".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.filler.counter.prev_cu".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.edge".into(), var_type: "R_TRIG".into() },
        CatalogEntry { name: "Main.filler.edge.CLK".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.edge.Q".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.filler.edge.prev".into(), var_type: "BOOL".into() },
        CatalogEntry { name: "Main.cycle".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array".into(), var_type: "ARRAY[0..9] OF INT".into() },
        CatalogEntry { name: "Main.test_array[0]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[1]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[2]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[3]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[4]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[5]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[6]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[7]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[8]".into(), var_type: "INT".into() },
        CatalogEntry { name: "Main.test_array[9]".into(), var_type: "INT".into() },
    ]);

    let addr = run_monitor_server("127.0.0.1:0", handle.clone())
        .await
        .expect("Failed to start monitor server");

    // Print port for the test harness
    println!("{}", addr.port());

    // Simulate scan cycles: push variable updates every 100ms
    let mut cycle: u64 = 0;
    loop {
        cycle += 1;
        let vars = vec![
            // Parent entries (for watch_tree builder)
            VariableValue { name: "Main.filler".into(), value: "".into(), var_type: "FillController".into(), forced: false },
            VariableValue { name: "Main.filler.counter".into(), value: "".into(), var_type: "CTU".into(), forced: false },
            VariableValue { name: "Main.filler.edge".into(), value: "".into(), var_type: "R_TRIG".into(), forced: false },
            VariableValue { name: "Main.test_array".into(), value: "".into(), var_type: "ARRAY[0..9] OF INT".into(), forced: false },
            // Scalar fields
            VariableValue { name: "Main.filler.start".into(), value: if cycle % 20 == 0 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.target_fill".into(), value: "5".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.filler.fill_done".into(), value: "FALSE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.fill_count".into(), value: format!("{}", (cycle / 2) % 6), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.filler.filling".into(), value: if cycle > 5 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            // Counter fields
            VariableValue { name: "Main.filler.counter.CU".into(), value: if cycle % 2 == 0 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.counter.RESET".into(), value: "FALSE".into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.counter.PV".into(), value: "5".into(), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.filler.counter.Q".into(), value: if cycle / 2 >= 5 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.counter.CV".into(), value: format!("{}", (cycle / 2) % 6), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.filler.counter.prev_cu".into(), value: if (cycle.wrapping_sub(1)) % 2 == 0 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            // Edge fields
            VariableValue { name: "Main.filler.edge.CLK".into(), value: if cycle % 20 == 0 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.edge.Q".into(), value: if cycle == 1 { "TRUE" } else { "FALSE" }.into(), var_type: "BOOL".into(), forced: false },
            VariableValue { name: "Main.filler.edge.prev".into(), value: "FALSE".into(), var_type: "BOOL".into(), forced: false },
            // Scalar
            VariableValue { name: "Main.cycle".into(), value: format!("{cycle}"), var_type: "INT".into(), forced: false },
            // Array elements
            VariableValue { name: "Main.test_array[0]".into(), value: format!("{cycle}"), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[1]".into(), value: format!("{}", cycle + 1), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[2]".into(), value: format!("{}", cycle + 2), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[3]".into(), value: format!("{}", cycle + 3), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[4]".into(), value: format!("{}", cycle + 4), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[5]".into(), value: format!("{}", cycle + 5), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[6]".into(), value: format!("{}", cycle + 6), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[7]".into(), value: format!("{}", cycle + 7), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[8]".into(), value: format!("{}", cycle + 8), var_type: "INT".into(), forced: false },
            VariableValue { name: "Main.test_array[9]".into(), value: format!("{}", cycle + 9), var_type: "INT".into(), forced: false },
        ];
        let cycle_info = CycleInfoData {
            cycle_count: cycle,
            last_cycle_us: 50,
            min_cycle_us: 30,
            max_cycle_us: 120,
            avg_cycle_us: 55,
            ..Default::default()
        };
        handle.update_variables(vars, cycle_info);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
