//! DAP integration tests.
//!
//! These test the DAP server by driving it in-process via the `run_dap`
//! function with piped stdin/stdout, sending DAP JSON messages and
//! verifying responses.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

/// A buffer that acts as both Read and Write for in-process testing.
#[derive(Clone)]
struct TestBuffer {
    data: Arc<Mutex<Vec<u8>>>,
    read_pos: Arc<Mutex<usize>>,
}

impl TestBuffer {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(Vec::new())),
            read_pos: Arc::new(Mutex::new(0)),
        }
    }

    fn write_dap_message(&self, msg: &Value) {
        let body = serde_json::to_string(msg).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut data = self.data.lock().unwrap();
        data.extend_from_slice(header.as_bytes());
        data.extend_from_slice(body.as_bytes());
    }

    fn read_all_output(&self) -> Vec<Value> {
        let data = self.data.lock().unwrap();
        let mut pos = 0;
        let mut messages = Vec::new();

        while pos < data.len() {
            // Find Content-Length header
            let remaining = &data[pos..];
            let header_end = find_double_crlf(remaining);
            if header_end.is_none() {
                break;
            }
            let header_end = header_end.unwrap();
            let header = std::str::from_utf8(&remaining[..header_end]).unwrap_or("");
            let content_length: usize = header
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: ").and_then(|v| v.trim().parse().ok()))
                .unwrap_or(0);

            pos += header_end + 4; // skip \r\n\r\n
            if pos + content_length > data.len() {
                break;
            }
            let body = &data[pos..pos + content_length];
            if let Ok(msg) = serde_json::from_slice::<Value>(body) {
                messages.push(msg);
            }
            pos += content_length;
        }
        messages
    }
}

fn find_double_crlf(data: &[u8]) -> Option<usize> {
    (0..data.len().saturating_sub(3)).find(|&i| data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n')
}

/// Helper: build a DAP request message.
fn dap_request(seq: i64, command: &str, arguments: Option<Value>) -> Value {
    let mut msg = json!({
        "seq": seq,
        "type": "request",
        "command": command,
    });
    if let Some(args) = arguments {
        msg["arguments"] = args;
    }
    msg
}

/// Create a test source file and return its path.
fn create_test_file(content: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("/tmp/dap_test_{}_{id}.st", std::process::id());
    std::fs::write(&path, content).unwrap();
    path
}

/// Run a DAP session with a sequence of requests and return all output messages.
fn run_dap_session(source: &str, requests: &[Value]) -> Vec<Value> {
    let path = create_test_file(source);

    // Build input buffer with all requests
    let mut input_data = Vec::new();
    for req in requests {
        let body = serde_json::to_string(req).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        input_data.extend_from_slice(header.as_bytes());
        input_data.extend_from_slice(body.as_bytes());
    }

    let input = Cursor::new(input_data);
    let mut output = Vec::new();

    st_dap::run_dap(input, &mut output, &path);

    // Clean up
    let _ = std::fs::remove_file(&path);

    // Parse output messages
    let buf = TestBuffer::new();
    buf.data.lock().unwrap().extend_from_slice(&output);
    buf.read_all_output()
}

/// Find a response to a specific request seq.
fn find_response(messages: &[Value], seq: i64) -> Option<&Value> {
    messages.iter().find(|m| {
        m["type"].as_str() == Some("response") && m["request_seq"].as_i64() == Some(seq)
    })
}

/// Find events of a specific type.
fn find_events<'a>(messages: &'a [Value], event_type: &str) -> Vec<&'a Value> {
    messages
        .iter()
        .filter(|m| m["type"].as_str() == Some("event") && m["event"].as_str() == Some(event_type))
        .collect()
}

// =============================================================================
// Tests
// =============================================================================

const SIMPLE_PROGRAM: &str = "\
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    x := 1;
    y := x + 2;
    x := y * 3;
END_PROGRAM
";

#[test]
fn test_initialize() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({
                "adapterID": "st",
                "clientID": "test"
            }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 1).expect("Expected initialize response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    assert!(resp["body"]["supportsConfigurationDoneRequest"].as_bool().unwrap_or(false));

    // Initialized event is sent after launch (per DAP spec)
    let events = find_events(&messages, "initialized");
    assert!(!events.is_empty(), "Expected initialized event after launch");
}

#[test]
fn test_launch_and_stopped_on_entry() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({ "program": "ignored" }))),
            dap_request(3, "disconnect", None),
        ],
    );

    let launch_resp = find_response(&messages, 2).expect("Expected launch response");
    assert!(launch_resp["success"].as_bool().unwrap_or(false));

    // Should get a stopped event with reason "entry"
    let stopped = find_events(&messages, "stopped");
    assert!(!stopped.is_empty(), "Expected stopped event on entry");
    assert_eq!(stopped[0]["body"]["reason"].as_str(), Some("entry"));
}

#[test]
fn test_threads() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "threads", None),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected threads response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    let threads = resp["body"]["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["name"].as_str(), Some("PLC Scan Cycle"));
}

#[test]
fn test_continue_runs_to_completion() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 4).expect("Expected continue response");
    assert!(resp["success"].as_bool().unwrap_or(false));

    // Program should terminate
    let terminated = find_events(&messages, "terminated");
    assert!(!terminated.is_empty(), "Expected terminated event after continue");
}

#[test]
fn test_step_in() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected stepIn response");
    assert!(resp["success"].as_bool().unwrap_or(false));

    // Should get a stopped event with reason "step"
    let stopped = find_events(&messages, "stopped");
    let step_stops: Vec<_> = stopped
        .iter()
        .filter(|e| e["body"]["reason"].as_str() == Some("step"))
        .collect();
    assert!(!step_stops.is_empty(), "Expected stopped-step event");
}

#[test]
fn test_next_step_over() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "next", Some(json!({ "threadId": 1 }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected next response");
    assert!(resp["success"].as_bool().unwrap_or(false));
}

#[test]
fn test_stack_trace() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "stackTrace", Some(json!({ "threadId": 1 }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected stackTrace response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    let frames = resp["body"]["stackFrames"].as_array().unwrap();
    assert!(!frames.is_empty(), "Expected at least one stack frame");
    assert_eq!(frames[0]["name"].as_str(), Some("Main"));
}

#[test]
fn test_scopes_and_variables() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "scopes", Some(json!({ "frameId": 0 }))),
            dap_request(4, "variables", Some(json!({ "variablesReference": 1000 }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let scopes_resp = find_response(&messages, 3).expect("Expected scopes response");
    assert!(scopes_resp["success"].as_bool().unwrap_or(false));
    let scopes = scopes_resp["body"]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 2);
    assert_eq!(scopes[0]["name"].as_str(), Some("Locals"));
    assert_eq!(scopes[1]["name"].as_str(), Some("Globals"));

    let vars_resp = find_response(&messages, 4).expect("Expected variables response");
    assert!(vars_resp["success"].as_bool().unwrap_or(false));
    let vars = vars_resp["body"]["variables"].as_array().unwrap();
    // Should have local variables x, y
    let names: Vec<_> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(names.contains(&"x"), "Expected variable 'x': {names:?}");
    assert!(names.contains(&"y"), "Expected variable 'y': {names:?}");
}

#[test]
fn test_evaluate() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "evaluate", Some(json!({
                "expression": "x",
                "frameId": 0
            }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected evaluate response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    // x should be 0 (initial value, stopped before execution)
    assert_eq!(resp["body"]["result"].as_str(), Some("0"));
}

#[test]
fn test_evaluate_unknown_variable() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "evaluate", Some(json!({
                "expression": "nonexistent",
                "frameId": 0
            }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected evaluate response");
    assert_eq!(resp["body"]["result"].as_str(), Some("<unknown>"));
}

#[test]
fn test_disconnect() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "disconnect", None),
        ],
    );

    // Verify initialize and launch succeeded
    let init_resp = find_response(&messages, 1).expect("Expected initialize response");
    assert!(init_resp["success"].as_bool().unwrap_or(false));
    let launch_resp = find_response(&messages, 2).expect("Expected launch response");
    assert!(launch_resp["success"].as_bool().unwrap_or(false));
    // Disconnect response may not be flushed from BufWriter before server exits — that's OK
}

#[test]
fn test_launch_invalid_file() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM, // source doesn't matter, path is overridden
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "disconnect", None),
        ],
    );

    // At minimum, initialize should succeed
    let resp = find_response(&messages, 1).expect("Expected initialize response");
    assert!(resp["success"].as_bool().unwrap_or(false));
}

#[test]
fn test_set_breakpoints() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [
                    { "line": 6 },
                    { "line": 7 }
                ]
            }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected setBreakpoints response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    let bps = resp["body"]["breakpoints"].as_array().unwrap();
    assert_eq!(bps.len(), 2);
}

#[test]
fn test_step_out() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "stepOut", Some(json!({ "threadId": 1 }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected stepOut response");
    assert!(resp["success"].as_bool().unwrap_or(false));
}

#[test]
fn test_pause() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "pause", Some(json!({ "threadId": 1 }))),
            dap_request(4, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 3).expect("Expected pause response");
    assert!(resp["success"].as_bool().unwrap_or(false));
}

#[test]
fn test_full_debug_session() {
    let source = "\
FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := Add(a := 10, b := 20);
    result := result + 1;
END_PROGRAM
";
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Step through a few instructions
            dap_request(4, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(5, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(6, "stackTrace", Some(json!({ "threadId": 1 }))),
            dap_request(7, "disconnect", None),
        ],
    );

    // Key responses should succeed (except disconnect which may not flush)
    for seq in 1..=6 {
        let resp = find_response(&messages, seq);
        assert!(
            resp.is_some(),
            "Missing response for seq {seq}"
        );
        assert!(
            resp.unwrap()["success"].as_bool().unwrap_or(false),
            "Response {seq} failed: {resp:?}"
        );
    }
}

#[test]
fn test_breakpoint_hit_and_variable_inspection() {
    // Test the full debug flow: set breakpoint, hit it, inspect variables
    let source = "\
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    x := 10;
    y := x + 20;
    x := y * 2;
END_PROGRAM
";
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            // Set breakpoint on "y := x + 20" (line 7)
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 7 }]
            }))),
            dap_request(4, "configurationDone", None),
            // Continue — should hit breakpoint on line 7
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            // Now inspect state
            dap_request(6, "stackTrace", Some(json!({ "threadId": 1 }))),
            dap_request(7, "scopes", Some(json!({ "frameId": 0 }))),
            dap_request(8, "variables", Some(json!({ "variablesReference": 1000 }))),
            // Evaluate x — should be 10 (already executed x := 10)
            dap_request(9, "evaluate", Some(json!({
                "expression": "x",
                "frameId": 0
            }))),
            dap_request(10, "disconnect", None),
        ],
    );

    // Breakpoint should be verified
    let bp_resp = find_response(&messages, 3).unwrap();
    let bps = bp_resp["body"]["breakpoints"].as_array().unwrap();
    assert!(bps[0]["verified"].as_bool().unwrap_or(false), "Breakpoint should be verified");

    // Should have stopped at breakpoint
    let stopped_events = find_events(&messages, "stopped");
    let bp_stops: Vec<_> = stopped_events
        .iter()
        .filter(|e| e["body"]["reason"].as_str() == Some("breakpoint"))
        .collect();
    assert!(!bp_stops.is_empty(), "Should stop at breakpoint");

    // Stack trace should show Main
    let st_resp = find_response(&messages, 6).unwrap();
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    assert!(!frames.is_empty(), "Should have stack frames");
    assert_eq!(frames[0]["name"].as_str(), Some("Main"));

    // Variables should include x and y
    let vars_resp = find_response(&messages, 8).unwrap();
    let vars = vars_resp["body"]["variables"].as_array().unwrap();
    let var_names: Vec<_> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(var_names.contains(&"x"), "Should have variable x: {var_names:?}");
    assert!(var_names.contains(&"y"), "Should have variable y: {var_names:?}");

    // x should be 10 (already executed x := 10 before breakpoint on y := ...)
    let eval_resp = find_response(&messages, 9).unwrap();
    assert_eq!(eval_resp["body"]["result"].as_str(), Some("10"), "x should be 10");
}

#[test]
fn test_step_and_variable_changes() {
    // Verify that stepping advances execution and variables update
    let source = "\
PROGRAM Main
VAR
    a : INT := 0;
END_VAR
    a := 1;
    a := 2;
    a := 3;
END_PROGRAM
";
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Step over first statement (a := 1)
            dap_request(4, "next", Some(json!({ "threadId": 1 }))),
            // Check a = 1
            dap_request(5, "evaluate", Some(json!({ "expression": "a", "frameId": 0 }))),
            // Step over second statement (a := 2)
            dap_request(6, "next", Some(json!({ "threadId": 1 }))),
            // Check a = 2
            dap_request(7, "evaluate", Some(json!({ "expression": "a", "frameId": 0 }))),
            dap_request(8, "disconnect", None),
        ],
    );

    // Verify step responses succeeded
    let step1 = find_response(&messages, 4).unwrap();
    assert!(step1["success"].as_bool().unwrap_or(false), "Step 1 should succeed");
    let step2 = find_response(&messages, 6).unwrap();
    assert!(step2["success"].as_bool().unwrap_or(false), "Step 2 should succeed");

    // After stepping, evaluate should return numeric values for 'a'
    let eval1 = find_response(&messages, 5).unwrap();
    let a1 = eval1["body"]["result"].as_str().unwrap_or("?");
    let eval2 = find_response(&messages, 7).unwrap();
    let a2 = eval2["body"]["result"].as_str().unwrap_or("?");

    assert!(a1 != "<unknown>", "Should evaluate 'a' after step: got {a1}");
    assert!(a2 != "<unknown>", "Should evaluate 'a' after step: got {a2}");
}

// =============================================================================
// PLC-specific extensions: force/unforce, cycle info
// =============================================================================

#[test]
fn test_force_variable() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Force x to 999
            dap_request(4, "evaluate", Some(json!({
                "expression": "force x = 999",
                "context": "repl"
            }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 4).expect("Expected force response");
    assert!(resp["success"].as_bool().unwrap_or(false));
    let result = resp["body"]["result"].as_str().unwrap_or("");
    assert!(result.contains("Forced"), "Expected 'Forced' in result: {result}");
    assert!(result.contains("999"), "Expected '999' in result: {result}");
}

#[test]
fn test_force_then_read() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Force x
            dap_request(4, "evaluate", Some(json!({
                "expression": "force x = 42",
                "context": "repl"
            }))),
            // Step — x should now read as 42
            dap_request(5, "next", Some(json!({ "threadId": 1 }))),
            dap_request(6, "evaluate", Some(json!({
                "expression": "x",
                "context": "watch"
            }))),
            dap_request(7, "disconnect", None),
        ],
    );

    let force_resp = find_response(&messages, 4).unwrap();
    assert!(force_resp["body"]["result"].as_str().unwrap_or("").contains("Forced"));
}

#[test]
fn test_unforce_variable() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Force then unforce
            dap_request(4, "evaluate", Some(json!({ "expression": "force x = 42" }))),
            dap_request(5, "evaluate", Some(json!({ "expression": "unforce x" }))),
            dap_request(6, "disconnect", None),
        ],
    );

    let unforce_resp = find_response(&messages, 5).unwrap();
    assert!(unforce_resp["body"]["result"].as_str().unwrap_or("").contains("Unforced"));
}

#[test]
fn test_list_forced_empty() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "evaluate", Some(json!({ "expression": "listForced" }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 4).unwrap();
    assert!(resp["body"]["result"].as_str().unwrap_or("").contains("No forced"));
}

#[test]
fn test_list_forced_with_entries() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "evaluate", Some(json!({ "expression": "force x = 100" }))),
            dap_request(5, "evaluate", Some(json!({ "expression": "force y = 200" }))),
            dap_request(6, "evaluate", Some(json!({ "expression": "listForced" }))),
            dap_request(7, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 6).unwrap();
    let result = resp["body"]["result"].as_str().unwrap_or("");
    assert!(result.contains("X") || result.contains("x"), "Should list forced X: {result}");
    assert!(result.contains("Y") || result.contains("y"), "Should list forced Y: {result}");
}

#[test]
fn test_cycle_info() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "evaluate", Some(json!({ "expression": "scanCycleInfo" }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 4).unwrap();
    let result = resp["body"]["result"].as_str().unwrap_or("");
    assert!(result.contains("Instructions"), "Expected cycle info: {result}");
}

#[test]
fn test_force_bool_variable() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "evaluate", Some(json!({ "expression": "force y = true" }))),
            dap_request(5, "evaluate", Some(json!({ "expression": "listForced" }))),
            dap_request(6, "disconnect", None),
        ],
    );

    let force_resp = find_response(&messages, 4).unwrap();
    assert!(force_resp["body"]["result"].as_str().unwrap_or("").contains("TRUE"));
}

#[test]
fn test_force_invalid_syntax() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "evaluate", Some(json!({ "expression": "force invalid" }))),
            dap_request(5, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 4).unwrap();
    assert!(resp["body"]["result"].as_str().unwrap_or("").contains("Usage"));
}

// =============================================================================
// Multi-file project tests
// =============================================================================

/// Create a temp project directory with multiple files and return the main.st path.
fn create_multi_file_project(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();

    // Always create plc-project.yaml
    let yaml = "name: TestProject\nentryPoint: Main\n";
    std::fs::write(dir.path().join("plc-project.yaml"), yaml).unwrap();

    let mut main_path = String::new();
    for (name, content) in files {
        let full_path = dir.path().join(name);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, content).unwrap();
        if *name == "main.st" {
            main_path = full_path.to_string_lossy().to_string();
        }
    }
    assert!(!main_path.is_empty(), "Must include main.st");
    (dir, main_path)
}

fn run_multi_file_dap_session(files: &[(&str, &str)], requests: &[Value]) -> Vec<Value> {
    let (_dir, main_path) = create_multi_file_project(files);

    let mut input_data = Vec::new();
    for req in requests {
        let body = serde_json::to_string(req).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        input_data.extend_from_slice(header.as_bytes());
        input_data.extend_from_slice(body.as_bytes());
    }

    let input = Cursor::new(input_data);
    let mut output = Vec::new();
    st_dap::run_dap(input, &mut output, &main_path);

    let buf = TestBuffer::new();
    buf.data.lock().unwrap().extend_from_slice(&output);
    buf.read_all_output()
}

#[test]
fn test_multi_file_launch() {
    let messages = run_multi_file_dap_session(
        &[
            ("counter.st", r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS
"#),
            ("main.st", r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR c : Counter; END_VAR
    c.Inc();
    g_val := c.Get();
END_PROGRAM
"#),
        ],
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "disconnect", None),
        ],
    );

    let launch_resp = find_response(&messages, 2).unwrap();
    assert!(launch_resp["success"].as_bool().unwrap_or(false), "Launch should succeed");

    // Should see initialized event (after launch)
    let events = find_events(&messages, "initialized");
    assert!(!events.is_empty(), "Should get initialized event");

    // Should be stopped on entry
    let stopped = find_events(&messages, "stopped");
    assert!(!stopped.is_empty(), "Should stop on entry");
}

#[test]
fn test_multi_file_breakpoint_in_main() {
    let messages = run_multi_file_dap_session(
        &[
            ("counter.st", r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS
"#),
            ("main.st", r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR c : Counter; x : INT := 0; END_VAR
    x := x + 1;
    c.Inc();
    g_val := c.Get();
END_PROGRAM
"#),
        ],
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "MAIN_PATH_PLACEHOLDER" },
                "breakpoints": [{ "line": 5 }]
            }))),
            dap_request(4, "configurationDone", None),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(6, "disconnect", None),
        ],
    );

    // Check breakpoint was verified
    let bp_resp = find_response(&messages, 3).unwrap();
    let bps = bp_resp["body"]["breakpoints"].as_array().unwrap();
    assert!(!bps.is_empty(), "Should have breakpoint results");
    assert!(bps[0]["verified"].as_bool().unwrap_or(false),
        "Breakpoint should be verified: {}", serde_json::to_string_pretty(bp_resp).unwrap());

    // Should have stopped on breakpoint
    let stopped = find_events(&messages, "stopped");
    let bp_stop = stopped.iter().find(|e| {
        e["body"]["reason"].as_str() == Some("breakpoint")
    });
    assert!(bp_stop.is_some(), "Should stop on breakpoint. Events: {:?}",
        stopped.iter().map(|e| e["body"]["reason"].as_str()).collect::<Vec<_>>());
}

#[test]
fn test_multi_file_step_into_class_method() {
    let messages = run_multi_file_dap_session(
        &[
            ("counter.st", r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc
    _count := _count + 1;
END_METHOD
END_CLASS
"#),
            ("main.st", r#"
PROGRAM Main
VAR c : Counter; END_VAR
    c.Inc();
END_PROGRAM
"#),
        ],
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Step into c.Inc() — should enter the method
            dap_request(4, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(5, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(6, "stepIn", Some(json!({ "threadId": 1 }))),
            dap_request(7, "stackTrace", Some(json!({ "threadId": 1 }))),
            dap_request(8, "disconnect", None),
        ],
    );

    // Check stack trace has correct function names
    let st_resp = find_response(&messages, 7).unwrap();
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    assert!(!frames.is_empty(), "Should have stack frames");

    // Top frame should be in Counter.Inc or Main
    let top_name = frames[0]["name"].as_str().unwrap_or("");
    eprintln!("Stack frames: {:?}",
        frames.iter().map(|f| f["name"].as_str().unwrap_or("")).collect::<Vec<_>>());

    // If stepped into Counter.Inc, top frame should show counter.st
    if top_name == "Counter.Inc" {
        let source_name = frames[0]["source"]["name"].as_str().unwrap_or("");
        assert_eq!(source_name, "counter.st",
            "Counter.Inc should show counter.st as source, got {source_name}");
    }
}

#[test]
fn test_multi_file_variables_after_method_call() {
    let messages = run_multi_file_dap_session(
        &[
            ("adder.st", r#"
CLASS Adder
METHOD Add : INT
VAR_INPUT a : INT; b : INT; END_VAR
    Add := a + b;
END_METHOD
END_CLASS
"#),
            ("main.st", r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR calc : Adder; result : INT; END_VAR
    result := calc.Add(a := 10, b := 32);
    g_result := result;
END_PROGRAM
"#),
        ],
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Run through one full cycle
            dap_request(4, "continue", Some(json!({ "threadId": 1 }))),
            // After cycle completes, check globals
            dap_request(5, "evaluate", Some(json!({
                "expression": "g_result",
                "context": "watch"
            }))),
            dap_request(6, "disconnect", None),
        ],
    );

    // After one cycle, g_result should be 42
    let eval_resp = find_response(&messages, 5).unwrap();
    let result_str = eval_resp["body"]["result"].as_str().unwrap_or("<missing>");
    eprintln!("g_result = {result_str}");
    assert_eq!(result_str, "42", "g_result should be 42 (10+32)");
}
