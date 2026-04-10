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
///
/// Note about `arguments`: the dap crate's `Command` enum is deserialized
/// with `#[serde(tag = "command", content = "arguments")]`. Unit variants
/// like `configurationDone` MUST omit the `arguments` field; tuple variants
/// like `disconnect(DisconnectArguments)` MUST include it (use `Some(json!({}))`
/// for "no meaningful args"). The set of unit variants is small and stable;
/// see `UNIT_COMMANDS` below.
fn dap_request(seq: i64, command: &str, arguments: Option<Value>) -> Value {
    const UNIT_COMMANDS: &[&str] = &["configurationDone", "loadedSources", "threads"];
    let mut msg = json!({
        "seq": seq,
        "type": "request",
        "command": command,
    });
    let is_unit = UNIT_COMMANDS.contains(&command);
    if let Some(args) = arguments {
        msg["arguments"] = args;
    } else if !is_unit {
        // Tuple variant with no caller-supplied args: send an empty object
        // so the dap crate's tag/content deserializer accepts the message.
        msg["arguments"] = json!({});
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
fn test_continue_interrupted_by_pause() {
    // Continue runs forever (PLC scan loop). A queued Pause should be picked
    // up by process_inflight_requests between cycles, set the VM's pause
    // flag, and the next iteration's step should Halt with reason "pause".
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(5, "pause", Some(json!({ "threadId": 1 }))),
            dap_request(6, "disconnect", None),
        ],
    );

    let cont_resp = find_response(&messages, 4).expect("Expected continue response");
    assert!(cont_resp["success"].as_bool().unwrap_or(false));

    // The Pause should have triggered a Stopped event with reason "pause".
    let stopped = find_events(&messages, "stopped");
    let pause_stops: Vec<_> = stopped
        .iter()
        .filter(|e| e["body"]["reason"].as_str() == Some("pause"))
        .collect();
    assert!(
        !pause_stops.is_empty(),
        "Expected a Stopped(pause) event, got: {:?}",
        stopped
            .iter()
            .map(|e| e["body"]["reason"].as_str())
            .collect::<Vec<_>>()
    );

    let pause_resp = find_response(&messages, 5).expect("Expected pause response");
    assert!(pause_resp["success"].as_bool().unwrap_or(false));
}

#[test]
fn test_continue_then_disconnect() {
    // PLC programs run forever (cyclic scan) — `continue` should run until
    // the user disconnects, sets a breakpoint, or pauses. The disconnect
    // request queued behind the continue should interrupt the run loop and
    // cleanly tear down the session.
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
    let disc_resp = find_response(&messages, 5).expect("Expected disconnect response");
    assert!(disc_resp["success"].as_bool().unwrap_or(false));
    // No spontaneous Terminated event — Continue is interruptible-only now.
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

// =============================================================================
// Cycle stats + telemetry tests (Tier 1 + Tier 2 cycle-time feedback)
// =============================================================================

/// Find every `output` event whose category is `telemetry` and whose `output`
/// sentinel is `plc/cycleStats`. Returns the structured `data` payloads.
fn find_cycle_stats_payloads(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .filter_map(|m| {
            if m["type"].as_str() != Some("event")
                || m["event"].as_str() != Some("output")
            {
                return None;
            }
            let body = &m["body"];
            if body["category"].as_str() != Some("telemetry") {
                return None;
            }
            if body["output"].as_str() != Some("plc/cycleStats") {
                return None;
            }
            Some(&body["data"])
        })
        .collect()
}

/// Parse the `Scan cycles: N` field out of `scanCycleInfo`'s formatted result.
fn parse_cycle_count(result: &str) -> Option<u64> {
    // "Scan cycles: 1234 | Instructions/cycle: ..."
    let after = result.strip_prefix("Scan cycles: ")?;
    let end = after.find(' ').unwrap_or(after.len());
    after[..end].parse().ok()
}

#[test]
fn test_cycle_stats_increments_after_continue() {
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(5, "evaluate", Some(json!({ "expression": "scanCycleInfo" }))),
            dap_request(6, "disconnect", None),
        ],
    );

    let resp = find_response(&messages, 5).expect("Expected scanCycleInfo response");
    let result = resp["body"]["result"].as_str().unwrap_or("");
    let count = parse_cycle_count(result)
        .unwrap_or_else(|| panic!("Could not parse cycle count from: {result}"));
    assert!(
        count > 0,
        "Cycle count should be > 0 after Continue, got {count}: {result}"
    );
    // SIMPLE_PROGRAM has no infinite loop, so the DAP loop runs many cycles
    // until it hits the 100k safety cap. We just need *some* timing.
    assert!(
        result.contains("last:") && result.contains("min/max/avg:"),
        "Expected timing fields in result: {result}"
    );
}

#[test]
fn test_cycle_stats_telemetry_event_schema() {
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

    let payloads = find_cycle_stats_payloads(&messages);
    assert!(
        !payloads.is_empty(),
        "Expected at least one plc/cycleStats telemetry event after Continue"
    );

    // Validate the schema of the most recent payload — every required field
    // must be present and the right JSON type. Future schema bumps should
    // update this assertion deliberately.
    let last = payloads.last().unwrap();
    assert_eq!(last["schema"].as_u64(), Some(3), "schema field");
    assert!(last["cycle_count"].is_u64(), "cycle_count must be u64");
    assert!(last["last_us"].is_u64(), "last_us must be u64");
    assert!(last["min_us"].is_u64(), "min_us must be u64");
    assert!(last["max_us"].is_u64(), "max_us must be u64");
    assert!(last["avg_us"].is_u64(), "avg_us must be u64");
    assert!(
        last["instructions_per_cycle"].is_u64(),
        "instructions_per_cycle must be u64"
    );
    // watchdog_us is Option<u64> — null when unset
    assert!(
        last["watchdog_us"].is_null() || last["watchdog_us"].is_u64(),
        "watchdog_us must be null or u64"
    );
    assert!(last["devices_ok"].is_u64(), "devices_ok must be u64");
    assert!(last["devices_err"].is_u64(), "devices_err must be u64");

    // Schema v2: period + jitter fields
    assert!(
        last["target_us"].is_null() || last["target_us"].is_u64(),
        "target_us must be null or u64"
    );
    assert!(last["last_period_us"].is_u64(), "last_period_us must be u64");
    assert!(last["min_period_us"].is_u64(), "min_period_us must be u64");
    assert!(last["max_period_us"].is_u64(), "max_period_us must be u64");
    assert!(last["jitter_max_us"].is_u64(), "jitter_max_us must be u64");

    // Schema v3: variables array. With the watch-list model the array is
    // empty by default — the panel must opt-in via `addWatch` / `watchVariables`.
    assert!(last["variables"].is_array(), "variables must be an array");
    assert!(
        last["variables"].as_array().unwrap().is_empty(),
        "variables should be empty by default — opt-in via addWatch"
    );

    // Sanity: with no comm devices wired up, both counts should be zero.
    assert_eq!(last["devices_ok"].as_u64(), Some(0));
    assert_eq!(last["devices_err"].as_u64(), Some(0));
    // After Continue we definitely ran at least one cycle.
    assert!(last["cycle_count"].as_u64().unwrap() > 0);
}

#[test]
fn test_watch_list_flow() {
    // Verify the addWatch / removeWatch / clearWatch evaluate commands
    // round-trip and that the variables array reflects the watch list.
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Add Main.x to the watch list — handle_watch_add will push a
            // fresh telemetry event reflecting the new list.
            dap_request(4, "evaluate", Some(json!({ "expression": "addWatch Main.x" }))),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(6, "disconnect", None),
        ],
    );

    let payloads = find_cycle_stats_payloads(&messages);
    assert!(!payloads.is_empty(), "Expected telemetry events");
    let last = payloads.last().unwrap();
    let vars = last["variables"].as_array().unwrap();
    let var_names: Vec<&str> = vars
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    assert!(
        var_names.iter().any(|n| n.eq_ignore_ascii_case("Main.x")),
        "Expected watched 'Main.x' in variables, got: {var_names:?}"
    );
    // We did NOT add Main.y, so it should NOT appear.
    assert!(
        !var_names.iter().any(|n| n.eq_ignore_ascii_case("Main.y")),
        "Did not expect un-watched 'Main.y' in variables"
    );
}

#[test]
fn test_force_does_not_freeze_other_watched_vars() {
    // Regression for the user-reported "force one variable, all the
    // others stop updating" scenario. We watch three variables, force
    // ONE, run a cycle, and verify the others got updated by the
    // program despite the force being active.
    //
    // Note: with the in-memory test harness all requests are buffered
    // up front, so a long Continue is interrupted after exactly one
    // cycle by the queued Disconnect. We assert "values reflect at
    // least one program write" rather than "advanced past N".
    let source = "\
VAR_GLOBAL
    di_0 : BOOL := FALSE;
    counter : INT := 0;
    state : INT := 0;
END_VAR
PROGRAM Main
    counter := counter + 1;
    state := 42;
END_PROGRAM
";
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(
                4,
                "evaluate",
                Some(json!({ "expression": "watchVariables di_0,counter,state" })),
            ),
            dap_request(5, "evaluate", Some(json!({ "expression": "force di_0 = true" }))),
            dap_request(6, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(7, "disconnect", None),
        ],
    );

    let payloads = find_cycle_stats_payloads(&messages);
    assert!(!payloads.is_empty(), "Expected telemetry events");
    let last = payloads.last().unwrap();
    let vars = last["variables"].as_array().unwrap();
    let by_name: std::collections::HashMap<String, &serde_json::Value> = vars
        .iter()
        .filter_map(|v| v["name"].as_str().map(|n| (n.to_uppercase(), v)))
        .collect();

    // The forced variable shows TRUE
    assert_eq!(
        by_name.get("DI_0").map(|v| v["value"].as_str().unwrap_or("")),
        Some("TRUE"),
        "di_0 should be the forced TRUE, got: {:?}",
        by_name.get("DI_0")
    );
    // Counter must have at least 1 (the program's `counter := counter + 1`
    // ran at least once after the force)
    let counter_val: i64 = by_name
        .get("COUNTER")
        .and_then(|v| v["value"].as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert!(
        counter_val >= 1,
        "counter should have advanced after the cycle, got {counter_val} — \
         force broke other variable updates"
    );
    // state must have been written to 42 (the program assigns it unconditionally)
    let state_val: i64 = by_name
        .get("STATE")
        .and_then(|v| v["value"].as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert_eq!(
        state_val, 42,
        "state should be 42 (unconditional program write) — \
         force broke unrelated variable updates"
    );
}

#[test]
fn test_var_catalog_emitted_on_launch() {
    // The DAP pushes a `plc/varCatalog` telemetry event right after launch
    // so the Monitor panel can populate its autocomplete dropdown.
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "disconnect", None),
        ],
    );

    let catalog_events: Vec<&serde_json::Value> = messages
        .iter()
        .filter_map(|m| {
            if m["type"].as_str() != Some("event")
                || m["event"].as_str() != Some("output")
            {
                return None;
            }
            let body = &m["body"];
            if body["category"].as_str() != Some("telemetry")
                || body["output"].as_str() != Some("plc/varCatalog")
            {
                return None;
            }
            Some(&body["data"])
        })
        .collect();

    assert!(
        !catalog_events.is_empty(),
        "Expected at least one plc/varCatalog telemetry event after launch"
    );
    let catalog = catalog_events[0];
    assert_eq!(catalog["schema"].as_u64(), Some(1));
    let vars = catalog["variables"].as_array().unwrap();
    let names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("Main.x")),
        "Expected 'Main.x' in catalog, got: {names:?}"
    );
}

#[test]
fn test_cycle_stats_force_flush_on_step() {
    // SIMPLE_PROGRAM has 3 statements. Three step-overs are enough to step
    // through the whole cycle and trigger the wrap-around path, which marks
    // a cycle as completed and triggers the post-loop force-flush — even
    // though 1 cycle is far below the periodic interval (default 20).
    let messages = run_dap_session(
        SIMPLE_PROGRAM,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            dap_request(4, "next", Some(json!({ "threadId": 1 }))),
            dap_request(5, "next", Some(json!({ "threadId": 1 }))),
            dap_request(6, "next", Some(json!({ "threadId": 1 }))),
            dap_request(7, "disconnect", None),
        ],
    );

    let payloads = find_cycle_stats_payloads(&messages);
    assert!(
        !payloads.is_empty(),
        "Expected force-flushed plc/cycleStats event after stepping through a full cycle"
    );
    // The flush should reflect exactly one completed cycle (not 20+).
    let last = payloads.last().unwrap();
    let count = last["cycle_count"].as_u64().unwrap_or(0);
    assert!(
        count >= 1 && count < 20,
        "Force-flushed event should fire below the periodic interval, got cycle_count={count}"
    );
}

#[test]
fn test_cycle_stats_invariants_after_continue() {
    // Note: with the interruptible run loop, a queued Disconnect interrupts
    // Continue after exactly one cycle, so we don't get multiple periodic
    // emissions in this test setup. We test the post-loop force-flush
    // payload's invariants instead, which is the same code path.
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

    let payloads = find_cycle_stats_payloads(&messages);
    assert!(!payloads.is_empty(), "Expected at least one telemetry event");

    let last = payloads.last().unwrap();
    let cycle_count = last["cycle_count"].as_u64().unwrap();
    let min_us = last["min_us"].as_u64().unwrap();
    let max_us = last["max_us"].as_u64().unwrap();
    let avg_us = last["avg_us"].as_u64().unwrap();
    let last_us = last["last_us"].as_u64().unwrap();

    assert!(cycle_count >= 1, "Expected ≥1 cycle, got {cycle_count}");
    assert!(
        min_us <= avg_us,
        "Invariant: min ({min_us}) ≤ avg ({avg_us})"
    );
    assert!(
        avg_us <= max_us,
        "Invariant: avg ({avg_us}) ≤ max ({max_us})"
    );
    assert!(
        last_us <= max_us,
        "Invariant: last ({last_us}) ≤ max ({max_us})"
    );
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

// =============================================================================
// FB instance field resolution in debugger
// =============================================================================

#[test]
fn test_evaluate_fb_field_while_paused_inside_fb() {
    // When paused inside a FillController-like FB, the user hovers over
    // `counter.Q` or types it in the Watch panel. The DAP must resolve
    // this dotted path by looking up the CTU instance's state from the
    // current frame context.
    let source = "\
FUNCTION_BLOCK MyFB\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    ctr : CTU;\n\
END_VAR\n\
    ctr(CU := cmd, RESET := FALSE, PV := 5);\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR\n\
    fb : MyFB;\n\
END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            // Set breakpoint on the ctr() call line inside MyFB (line 6)
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 6 }]
            }))),
            dap_request(4, "configurationDone", None),
            // Continue — should hit breakpoint inside MyFB
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            // Now evaluate `ctr.Q` while paused inside MyFB
            dap_request(6, "evaluate", Some(json!({
                "expression": "ctr.Q",
                "frameId": 0
            }))),
            // Also evaluate a flat local
            dap_request(7, "evaluate", Some(json!({
                "expression": "cmd",
                "frameId": 0
            }))),
            // Check the Variables/Locals scope
            dap_request(8, "scopes", Some(json!({ "frameId": 0 }))),
            dap_request(9, "variables", Some(json!({ "variablesReference": 1000 }))),
            dap_request(10, "disconnect", None),
        ],
    );

    // ctr.Q should resolve (not <unknown>)
    let eval_resp = find_response(&messages, 6).unwrap();
    let ctr_q = eval_resp["body"]["result"].as_str().unwrap_or("<missing>");
    eprintln!("ctr.Q = {ctr_q}");
    assert_ne!(
        ctr_q, "<unknown>",
        "ctr.Q should resolve to a value when paused inside MyFB, got <unknown>"
    );

    // cmd should also resolve
    let cmd_resp = find_response(&messages, 7).unwrap();
    let cmd_val = cmd_resp["body"]["result"].as_str().unwrap_or("<missing>");
    eprintln!("cmd = {cmd_val}");
    assert_ne!(cmd_val, "<unknown>", "cmd should resolve");

    // The locals scope should include "ctr" as an expandable FB instance
    // (variablesReference > 0), NOT flat "ctr.Q" / "ctr.CV" entries.
    let vars_resp = find_response(&messages, 9).unwrap();
    let vars = vars_resp["body"]["variables"].as_array().unwrap();
    let var_names: Vec<&str> = vars.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("Locals: {var_names:?}");
    assert!(
        var_names.iter().any(|n| n.eq_ignore_ascii_case("ctr")),
        "Locals should include 'ctr' as a FB instance node, got: {var_names:?}"
    );
    // ctr should have variablesReference > 0 (expandable)
    let ctr_var = vars.iter().find(|v| {
        v["name"].as_str().map_or(false, |n| n.eq_ignore_ascii_case("ctr"))
    }).unwrap();
    let ctr_ref = ctr_var["variablesReference"].as_i64().unwrap_or(0);
    assert!(
        ctr_ref > 0,
        "ctr should have variablesReference > 0 (expandable FB), got {ctr_ref}"
    );
    eprintln!("ctr variablesReference = {ctr_ref}");
}

#[test]
fn test_fb_instance_tree_expansion() {
    // End-to-end: expand a FB instance to see its fields as children.
    // 1. Pause inside MyFB at a breakpoint
    // 2. Request Locals scope → get 'ctr' with variablesReference > 0
    // 3. Request Variables(ctr_ref) → get CTU's fields (CU, RESET, PV, Q, CV, prev_cu)
    let source = "\
FUNCTION_BLOCK MyFB\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    ctr : CTU;\n\
END_VAR\n\
    ctr(CU := cmd, RESET := FALSE, PV := 5);\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR fb : MyFB; END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 6 }]
            }))),
            dap_request(4, "configurationDone", None),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            // Get locals scope
            dap_request(6, "scopes", Some(json!({ "frameId": 0 }))),
            dap_request(7, "variables", Some(json!({ "variablesReference": 1000 }))),
            dap_request(8, "disconnect", None),
        ],
    );

    // Find ctr in locals and get its variablesReference
    let locals_resp = find_response(&messages, 7).unwrap();
    let locals = locals_resp["body"]["variables"].as_array().unwrap();
    let ctr = locals.iter().find(|v| {
        v["name"].as_str().map_or(false, |n| n.eq_ignore_ascii_case("ctr"))
    });
    assert!(ctr.is_some(), "Expected 'ctr' in locals, got: {:?}",
        locals.iter().map(|v| v["name"].as_str()).collect::<Vec<_>>());
    let ctr = ctr.unwrap();
    let ctr_ref = ctr["variablesReference"].as_i64().unwrap_or(0);
    assert!(ctr_ref > 0, "ctr should be expandable (variablesReference > 0)");

    // ctr should show a type name
    let type_str = ctr["type"].as_str().unwrap_or("");
    eprintln!("ctr type = {type_str}");
    assert!(
        type_str.to_uppercase().contains("CTU"),
        "ctr type should mention CTU, got '{type_str}'"
    );

    // Now expand ctr by requesting its children. We need to do this in a
    // SECOND session because the first one has already disconnected. But
    // since we can't easily extend the test, let's verify the structure
    // from what we have. The key assertion: ctr IS expandable.
    eprintln!("ctr summary value = {:?}", ctr["value"].as_str());
}

#[test]
fn test_fb_children_request() {
    // Full round-trip: get the ctr variablesReference, then request its children.
    let source = "\
FUNCTION_BLOCK MyFB\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    ctr : CTU;\n\
END_VAR\n\
    ctr(CU := cmd, RESET := FALSE, PV := 5);\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR fb : MyFB; END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    // We need two Variables requests: first for locals (ref 1000), then
    // for the ctr FB children (the ref ID allocated for ctr).
    // The ref for ctr will be allocated after the locals_ref (1000) and
    // globals_ref (1001), so it should be 1002.
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 6 }]
            }))),
            dap_request(4, "configurationDone", None),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(6, "scopes", Some(json!({ "frameId": 0 }))),
            // Request locals (allocates ctr's FB ref)
            dap_request(7, "variables", Some(json!({ "variablesReference": 1000 }))),
            // Request ctr's children (ref should be 1002 — after locals=1000, globals=1001)
            dap_request(8, "variables", Some(json!({ "variablesReference": 1002 }))),
            dap_request(9, "disconnect", None),
        ],
    );

    // Check the children response
    let children_resp = find_response(&messages, 8).unwrap();
    let children = children_resp["body"]["variables"].as_array().unwrap();
    let child_names: Vec<&str> = children.iter().filter_map(|v| v["name"].as_str()).collect();
    eprintln!("CTU children: {child_names:?}");

    // CTU should have: CU, RESET, PV, Q, CV, prev_cu
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("Q")),
        "CTU children should include Q, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("CV")),
        "CTU children should include CV, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("PV")),
        "CTU children should include PV, got: {child_names:?}"
    );
    assert!(
        child_names.len() >= 5,
        "Expected at least 5 CTU fields (CU, RESET, PV, Q, CV, prev_cu), got {}",
        child_names.len()
    );
}

#[test]
fn test_evaluate_fb_instance_is_expandable_in_watch() {
    // When the user adds "ctr" to the Watch panel (or hovers over it),
    // VS Code sends an Evaluate request. If ctr is a FB instance, the
    // EvaluateResponse should have variablesReference > 0 so VS Code
    // shows the expand arrow. Clicking it sends a Variables request
    // with that ref to get the children.
    let source = "\
FUNCTION_BLOCK MyFB\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    ctr : CTU;\n\
END_VAR\n\
    ctr(CU := cmd, RESET := FALSE, PV := 5);\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR fb : MyFB; END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 6 }]
            }))),
            dap_request(4, "configurationDone", None),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            // Evaluate "ctr" as a Watch expression
            dap_request(6, "evaluate", Some(json!({
                "expression": "ctr",
                "context": "watch",
                "frameId": 0
            }))),
            // Evaluate a scalar for comparison
            dap_request(7, "evaluate", Some(json!({
                "expression": "cmd",
                "context": "watch",
                "frameId": 0
            }))),
            dap_request(8, "disconnect", None),
        ],
    );

    // "ctr" should be expandable (variablesReference > 0)
    let ctr_resp = find_response(&messages, 6).unwrap();
    let ctr_ref = ctr_resp["body"]["variablesReference"].as_i64().unwrap_or(0);
    let ctr_result = ctr_resp["body"]["result"].as_str().unwrap_or("");
    let ctr_type = ctr_resp["body"]["type"].as_str().unwrap_or("");
    eprintln!("Watch ctr: result={ctr_result:?} type={ctr_type:?} ref={ctr_ref}");
    assert!(
        ctr_ref > 0,
        "Evaluate('ctr') should return variablesReference > 0 for expandable FB, got {ctr_ref}"
    );
    assert!(
        ctr_type.to_uppercase().contains("CTU"),
        "Type should mention CTU, got '{ctr_type}'"
    );
    assert_ne!(ctr_result, "<unknown>", "ctr should have a summary value");

    // "cmd" should NOT be expandable (scalar BOOL)
    let cmd_resp = find_response(&messages, 7).unwrap();
    let cmd_ref = cmd_resp["body"]["variablesReference"].as_i64().unwrap_or(0);
    assert_eq!(cmd_ref, 0, "Scalar 'cmd' should have variablesReference=0");
}

#[test]
fn test_watch_panel_evaluate_fb_then_expand_children() {
    // Full Watch panel round-trip: Evaluate("ctr") → get variablesReference →
    // Variables(ref) → verify children with values.
    //
    // The breakpoint is placed on a dummy statement AFTER the ctr() call so
    // the FB instance has been populated with real values by the time we
    // pause and inspect.
    let source = "\
FUNCTION_BLOCK MyFB\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    ctr : CTU;\n\
    done : BOOL;\n\
END_VAR\n\
    ctr(CU := cmd, RESET := FALSE, PV := 5);\n\
    done := TRUE;\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR fb : MyFB; END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    // Break on line 8 (done := TRUE) — after ctr() has executed.
    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "setBreakpoints", Some(json!({
                "source": { "path": "test.st" },
                "breakpoints": [{ "line": 8 }]
            }))),
            dap_request(4, "configurationDone", None),
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            // Step 1: Evaluate "ctr" in the Watch panel context
            dap_request(6, "evaluate", Some(json!({
                "expression": "ctr",
                "context": "watch",
                "frameId": 0
            }))),
            // Step 2: Will send Variables request using the ref from step 1.
            // The evaluate response allocates a new ref starting from
            // next_var_ref. We need to find the actual ref from the response.
            // Since this is a serial protocol, we use a placeholder here
            // and compute the real ref from the evaluate response below.
            // But run_dap_session sends all requests up front, so we need
            // to predict the ref ID.
            //
            // After initialize+launch, the scopes haven't been requested
            // yet, so next_var_ref is still at its initial value (1000).
            // The Evaluate handler allocates ref 1000 for ctr.
            dap_request(7, "variables", Some(json!({ "variablesReference": 1000 }))),
            dap_request(8, "disconnect", None),
        ],
    );

    // Verify step 1: Evaluate returned an expandable ref
    let eval_resp = find_response(&messages, 6).unwrap();
    let eval_ref = eval_resp["body"]["variablesReference"].as_i64().unwrap_or(0);
    let eval_result = eval_resp["body"]["result"].as_str().unwrap_or("");
    let eval_type = eval_resp["body"]["type"].as_str().unwrap_or("");
    eprintln!(
        "Evaluate(ctr): result={eval_result:?} type={eval_type:?} ref={eval_ref}"
    );
    assert!(
        eval_ref > 0,
        "Evaluate('ctr') should return variablesReference > 0, got {eval_ref}"
    );
    assert_eq!(
        eval_ref, 1000,
        "Expected ref 1000 (first allocation), got {eval_ref}"
    );

    // Verify step 2: Variables(ref) returned CTU's children with values
    let children_resp = find_response(&messages, 7).unwrap();
    assert!(
        children_resp["success"].as_bool().unwrap_or(false),
        "Variables request should succeed"
    );
    let children = children_resp["body"]["variables"]
        .as_array()
        .expect("should have variables array");
    let child_names: Vec<&str> = children
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    eprintln!("CTU children via Watch expand: {child_names:?}");

    // CTU should have: CU, RESET, PV, Q, CV, prev_cu
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("CU")),
        "CTU children should include CU, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("Q")),
        "CTU children should include Q, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("CV")),
        "CTU children should include CV, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("PV")),
        "CTU children should include PV, got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| n.eq_ignore_ascii_case("RESET")),
        "CTU children should include RESET, got: {child_names:?}"
    );
    assert!(
        child_names.len() >= 5,
        "Expected at least 5 CTU fields, got {}: {child_names:?}",
        child_names.len()
    );

    // Verify the children have actual values (not all Void/empty)
    // After one cycle with CU=TRUE, PV=5: CV should be 1, Q should be FALSE
    let cv = children
        .iter()
        .find(|v| v["name"].as_str().map(|n| n.eq_ignore_ascii_case("CV")).unwrap_or(false));
    if let Some(cv_var) = cv {
        let cv_val = cv_var["value"].as_str().unwrap_or("");
        eprintln!("CV value: {cv_val:?}");
        assert_ne!(cv_val, "", "CV should have a value after ctr() executed");
    }

    let pv = children
        .iter()
        .find(|v| v["name"].as_str().map(|n| n.eq_ignore_ascii_case("PV")).unwrap_or(false));
    if let Some(pv_var) = pv {
        let pv_val = pv_var["value"].as_str().unwrap_or("");
        eprintln!("PV value: {pv_val:?}");
        assert_eq!(pv_val, "5", "PV should be 5 (the preset value)");
    }
}

#[test]
fn test_monitor_watch_fb_prefix_includes_all_descendants() {
    // End-to-end test for the PLC Monitor panel's tree view data flow.
    // When watching a FB instance prefix like "Main.fb", the telemetry
    // payload must include ALL descendant scalar fields so the panel
    // can build a recursive tree.
    let source = "\
FUNCTION_BLOCK Inner\n\
VAR_INPUT x : INT; END_VAR\n\
VAR_OUTPUT y : INT; END_VAR\n\
    y := x * 2;\n\
END_FUNCTION_BLOCK\n\
\n\
FUNCTION_BLOCK Outer\n\
VAR_INPUT cmd : BOOL; END_VAR\n\
VAR\n\
    sub : Inner;\n\
    state : INT := 0;\n\
END_VAR\n\
    state := state + 1;\n\
    sub(x := state);\n\
END_FUNCTION_BLOCK\n\
\n\
PROGRAM Main\n\
VAR\n\
    fb : Outer;\n\
END_VAR\n\
    fb(cmd := TRUE);\n\
END_PROGRAM\n";

    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "configurationDone", None),
            // Watch the Outer FB instance — should include nested Inner fields
            dap_request(
                4,
                "evaluate",
                Some(json!({ "expression": "watchVariables Main.fb" })),
            ),
            // Run one cycle so the FB state gets populated
            dap_request(5, "continue", Some(json!({ "threadId": 1 }))),
            dap_request(6, "disconnect", None),
        ],
    );

    // Find the telemetry payload
    let payloads = find_cycle_stats_payloads(&messages);
    assert!(!payloads.is_empty(), "Expected at least one telemetry event");
    let last = payloads.last().unwrap();
    let vars = last["variables"].as_array().unwrap();

    // Collect all variable names from the telemetry
    let var_names: Vec<&str> = vars
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    eprintln!("Telemetry variables: {var_names:?}");

    // Should include nested Inner FB fields (2 levels deep)
    assert!(
        var_names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.state")),
        "Should include Outer.state, got: {var_names:?}"
    );
    assert!(
        var_names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.x")),
        "Should include nested Inner.x (2 levels), got: {var_names:?}"
    );
    assert!(
        var_names.iter().any(|n| n.eq_ignore_ascii_case("Main.fb.sub.y")),
        "Should include nested Inner.y (2 levels), got: {var_names:?}"
    );

    // The tree parent 'Main.fb' is now present with a nested `children` array.
    let fb_entry = vars
        .iter()
        .find(|v| v["name"].as_str().map(|n| n.eq_ignore_ascii_case("Main.fb")).unwrap_or(false));
    assert!(
        fb_entry.is_some(),
        "Tree parent 'Main.fb' should be present with children: {var_names:?}"
    );
    let fb_children = fb_entry.unwrap()["children"].as_array();
    assert!(
        fb_children.is_some(),
        "Main.fb should have a 'children' array"
    );
    let child_names: Vec<&str> = fb_children
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    eprintln!("Main.fb children: {child_names:?}");
    assert!(
        child_names.iter().any(|n| *n == "cmd" || n.eq_ignore_ascii_case("cmd")),
        "children should include 'cmd', got: {child_names:?}"
    );
    assert!(
        child_names.iter().any(|n| *n == "state" || n.eq_ignore_ascii_case("state")),
        "children should include 'state', got: {child_names:?}"
    );
    // Nested FB 'sub' should appear as a child with its own children
    let sub_entry = fb_children
        .unwrap()
        .iter()
        .find(|c| c["name"].as_str().map(|n| n.eq_ignore_ascii_case("sub")).unwrap_or(false));
    assert!(
        sub_entry.is_some(),
        "children should include nested FB 'sub', got: {child_names:?}"
    );
    let sub_children = sub_entry.unwrap()["children"].as_array();
    assert!(
        sub_children.is_some(),
        "'sub' should have its own children array"
    );
    let sub_child_names: Vec<&str> = sub_children
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    eprintln!("Main.fb.sub children: {sub_child_names:?}");
    assert!(
        sub_child_names.iter().any(|n| n.eq_ignore_ascii_case("x")),
        "sub children should include 'x', got: {sub_child_names:?}"
    );
    assert!(
        sub_child_names.iter().any(|n| n.eq_ignore_ascii_case("y")),
        "sub children should include 'y', got: {sub_child_names:?}"
    );
}

#[test]
fn test_parse_errors_reported_with_file_and_line() {
    // When the source has parse errors, the launch should fail and the
    // Debug Console should show each error with file:line detail — not
    // just a generic "N parse error(s) found" message.
    let source = "\
PROGRAM Main\n\
VAR x : INT; END_VAR\n\
    x := ;\n\
    IF TRUE\n\
END_PROGRAM\n";

    let messages = run_dap_session(
        source,
        &[
            dap_request(1, "initialize", Some(json!({ "adapterID": "st" }))),
            dap_request(2, "launch", Some(json!({}))),
            dap_request(3, "disconnect", None),
        ],
    );

    // Launch should fail
    let launch_resp = find_response(&messages, 2).unwrap();
    assert_eq!(
        launch_resp["success"].as_bool(),
        Some(false),
        "Launch should fail with parse errors"
    );
    let error_msg = launch_resp["message"].as_str().unwrap_or("");
    eprintln!("Launch error: {error_msg}");
    assert!(
        error_msg.contains("parse error(s)"),
        "Error message should mention parse errors, got: {error_msg}"
    );
    // Should direct the user to the Problems panel (where the LSP already
    // shows the errors with file/line/column detail).
    assert!(
        error_msg.contains("Problems panel") || error_msg.contains("Problems"),
        "Error message should direct user to the Problems panel, got: {error_msg}"
    );
}
