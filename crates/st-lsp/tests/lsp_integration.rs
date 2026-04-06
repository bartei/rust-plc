//! LSP integration tests.
//!
//! Spawns `st-cli serve` as a subprocess and communicates with it via
//! JSON-RPC over stdin/stdout, exactly like a real editor would.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Test client that talks to the LSP server via JSON-RPC over stdin/stdout.
struct TestClient {
    child: Child,
    request_id: i64,
}

impl TestClient {
    fn start() -> Self {
        let bin = std::env::var("ST_CLI_BIN")
            .unwrap_or_else(|_| {
                let manifest_dir = env!("CARGO_MANIFEST_DIR");
                let workspace_root = std::path::Path::new(manifest_dir)
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap();
                // Try multiple target directories (normal build, llvm-cov, etc.)
                let candidates = [
                    workspace_root.join("target/debug/st-cli"),
                    workspace_root.join("target/llvm-cov-target/debug/st-cli"),
                ];
                candidates
                    .iter()
                    .find(|p| p.exists())
                    .unwrap_or(&candidates[0])
                    .to_string_lossy()
                    .to_string()
            });

        let child = Command::new(&bin)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("Failed to spawn '{bin}': {e}"));

        Self {
            child,
            request_id: 0,
        }
    }

    fn send(&mut self, msg: &Value) {
        let body = serde_json::to_string(msg).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(header.as_bytes()).unwrap();
        stdin.write_all(body.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.request_id += 1;
        let id = self.request_id;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }));
        // Read messages until we find the response
        loop {
            let msg = self.read_one();
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return msg;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }));
    }

    fn wait_for_notification(&mut self, method: &str) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "Timeout waiting for '{method}'"
            );
            let msg = self.read_one();
            if msg.get("method").and_then(|m| m.as_str()) == Some(method) {
                return msg;
            }
        }
    }

    fn read_one(&mut self) -> Value {
        let stdout = self.child.stdout.as_mut().unwrap();

        // Read headers byte-by-byte to avoid buffering issues
        let mut header = Vec::new();
        let mut found_double_crlf = false;
        while !found_double_crlf {
            let mut byte = [0u8; 1];
            stdout.read_exact(&mut byte).expect("EOF from LSP server");
            header.push(byte[0]);
            let len = header.len();
            if len >= 4
                && header[len - 4] == b'\r'
                && header[len - 3] == b'\n'
                && header[len - 2] == b'\r'
                && header[len - 1] == b'\n'
            {
                found_double_crlf = true;
            }
        }

        let header_str = String::from_utf8_lossy(&header);
        let content_length: usize = header_str
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .and_then(|v| v.trim().parse().ok())
            })
            .expect("Missing Content-Length header");

        let mut body = vec![0u8; content_length];
        stdout.read_exact(&mut body).expect("EOF reading body");

        serde_json::from_slice(&body).expect("Invalid JSON from server")
    }

    fn shutdown(mut self) {
        let _ = self.request("shutdown", json!(null));
        self.notify("exit", json!(null));
        // Give it a moment to exit cleanly
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn file_uri(name: &str) -> String {
    format!("file:///test/{name}")
}

// =============================================================================
// Tests
// =============================================================================

#[test]
fn test_initialize_and_capabilities() {
    let mut client = TestClient::start();
    let resp = client.request(
        "initialize",
        json!({
            "processId": null,
            "capabilities": {},
            "rootUri": "file:///test"
        }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected result object: {resp:?}");

    let caps = &result["capabilities"];
    // Check that our key capabilities are advertised
    assert!(caps["hoverProvider"].as_bool().unwrap_or(false));
    assert!(caps["textDocumentSync"].is_object() || caps["textDocumentSync"].is_number());
    assert!(caps["definitionProvider"].is_boolean() || caps["definitionProvider"].is_object());
    assert!(caps["semanticTokensProvider"].is_object());

    client.notify("initialized", json!({}));
    client.shutdown();
}

#[test]
fn test_diagnostics_on_open_clean_file() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("clean.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n"
            }
        }),
    );

    let notif = client.wait_for_notification("textDocument/publishDiagnostics");
    let params = &notif["params"];
    assert_eq!(params["uri"].as_str().unwrap(), uri);
    // Clean file should have zero errors (may have warnings)
    let diags = params["diagnostics"].as_array().unwrap();
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d["severity"].as_i64() == Some(1)) // 1 = Error
        .collect();
    assert!(
        errors.is_empty(),
        "Expected no errors in clean file, got: {errors:?}"
    );

    client.shutdown();
}

#[test]
fn test_diagnostics_on_open_broken_file() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("broken.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared;\nEND_PROGRAM\n"
            }
        }),
    );

    let notif = client.wait_for_notification("textDocument/publishDiagnostics");
    let diags = notif["params"]["diagnostics"].as_array().unwrap();
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d["severity"].as_i64() == Some(1))
        .collect();
    assert!(
        !errors.is_empty(),
        "Expected errors for undeclared variable"
    );
    // Check that the error message mentions 'undeclared'
    let has_undeclared = errors
        .iter()
        .any(|e| e["message"].as_str().unwrap_or("").contains("undeclared"));
    assert!(has_undeclared, "Expected 'undeclared' in error: {errors:?}");
}

#[test]
fn test_diagnostics_update_on_change() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("changing.st");

    // Open with error
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared;\nEND_PROGRAM\n"
            }
        }),
    );
    let notif = client.wait_for_notification("textDocument/publishDiagnostics");
    let errors: Vec<_> = notif["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"].as_i64() == Some(1))
        .collect();
    assert!(!errors.is_empty(), "v1 should have errors");

    // Fix the error
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 42;\nEND_PROGRAM\n"
            }]
        }),
    );
    let notif2 = client.wait_for_notification("textDocument/publishDiagnostics");
    let errors2: Vec<_> = notif2["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"].as_i64() == Some(1))
        .collect();
    assert!(
        errors2.is_empty(),
        "v2 should have no errors after fix, got: {errors2:?}"
    );

    client.shutdown();
}

#[test]
fn test_hover() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("hover.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + 1;\nEND_PROGRAM\n"
            }
        }),
    );
    // Consume diagnostics notification
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Hover over 'counter' on the assignment line (line 4, col 4)
    let resp = client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 6 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected hover result, got: {resp:?}");
    let contents = &result["contents"];
    let value = contents["value"].as_str().unwrap_or("");
    assert!(
        value.contains("INT"),
        "Hover should show type INT, got: {value}"
    );

    client.shutdown();
}

#[test]
fn test_goto_definition() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("gotodef.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    myVar : INT := 0;\nEND_VAR\n    myVar := myVar + 1;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Go-to-definition on 'myVar' in the assignment (line 4, col 14 — the usage on the right side)
    let resp = client.request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 14 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected definition result, got: {resp:?}");
    assert_eq!(result["uri"].as_str().unwrap(), uri);
    // Should point to the declaration on line 2
    let range = &result["range"];
    assert_eq!(
        range["start"]["line"].as_i64().unwrap(),
        2,
        "Definition should be on line 2 (VAR declaration)"
    );

    client.shutdown();
}

#[test]
fn test_semantic_tokens() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("tokens.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": uri } }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected semantic tokens result, got: {resp:?}");
    let data = result["data"].as_array().unwrap();
    // Semantic tokens are encoded as groups of 5 u32s
    assert!(
        data.len() >= 5,
        "Expected at least one token (5 values), got {} values",
        data.len()
    );
    assert_eq!(
        data.len() % 5,
        0,
        "Token data length must be a multiple of 5, got {}",
        data.len()
    );

    client.shutdown();
}

#[test]
fn test_document_close_clears_diagnostics() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("closeme.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared;\nEND_PROGRAM\n"
            }
        }),
    );
    // First notification: diagnostics with errors
    let notif1 = client.wait_for_notification("textDocument/publishDiagnostics");
    assert!(
        !notif1["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Close the document
    client.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": uri } }),
    );

    // Should get empty diagnostics
    let notif2 = client.wait_for_notification("textDocument/publishDiagnostics");
    assert_eq!(
        notif2["params"]["diagnostics"].as_array().unwrap().len(),
        0,
        "Diagnostics should be cleared on close"
    );

    client.shutdown();
}

#[test]
fn test_multiple_documents() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri1 = file_uri("doc1.st");
    let uri2 = file_uri("doc2.st");

    // Open two documents
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri1,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM P1\nVAR\n    a : INT := 0;\nEND_VAR\n    a := 1;\nEND_PROGRAM\n"
            }
        }),
    );
    let n1 = client.wait_for_notification("textDocument/publishDiagnostics");
    assert_eq!(n1["params"]["uri"].as_str().unwrap(), uri1);

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri2,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM P2\nVAR\n    b : BOOL := TRUE;\nEND_VAR\n    b := FALSE;\nEND_PROGRAM\n"
            }
        }),
    );
    let n2 = client.wait_for_notification("textDocument/publishDiagnostics");
    assert_eq!(n2["params"]["uri"].as_str().unwrap(), uri2);

    // Hover on first document should still work
    let resp = client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": uri1 },
            "position": { "line": 4, "character": 4 }
        }),
    );
    assert!(resp["result"].is_object());

    client.shutdown();
}

#[test]
fn test_completion_variables_and_keywords() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("completion.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    counter : INT := 0;\n    count_max : INT := 100;\nEND_VAR\n    counter := co;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 5, "character": 17 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected completion array, got: {resp:?}");
    let items = result.as_array().unwrap();
    assert!(!items.is_empty(), "Expected completion items");

    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(
        labels.iter().any(|l| l.eq_ignore_ascii_case("counter")),
        "Expected 'counter' in completions: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.eq_ignore_ascii_case("count_max")),
        "Expected 'count_max' in completions: {labels:?}"
    );

    client.shutdown();
}

#[test]
fn test_completion_dot_trigger_struct_fields() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("dotcompl.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "TYPE\n    Point : STRUCT\n        x : REAL := 0.0;\n        y : REAL := 0.0;\n    END_STRUCT;\nEND_TYPE\n\nPROGRAM Main\nVAR\n    p : Point;\n    val : REAL := 0.0;\nEND_VAR\n    val := p.x;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 12, "character": 13 },
            "context": { "triggerKind": 2, "triggerCharacter": "." }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected completion array for dot, got: {resp:?}");
    let items = result.as_array().unwrap();
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(
        labels.iter().any(|l| l.eq_ignore_ascii_case("x")),
        "Expected 'x' field in struct completions: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.eq_ignore_ascii_case("y")),
        "Expected 'y' field in struct completions: {labels:?}"
    );

    client.shutdown();
}

#[test]
fn test_completion_function_snippet() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("funccompl.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Ad\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 12, "character": 16 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected completion array, got: {resp:?}");
    let items = result.as_array().unwrap();
    let add_item = items.iter().find(|i| i["label"].as_str() == Some("Add"));
    assert!(add_item.is_some(), "Expected 'Add' function in completions");

    let add = add_item.unwrap();
    assert_eq!(add["insertTextFormat"].as_i64(), Some(2)); // 2 = Snippet
    let insert_text = add["insertText"].as_str().unwrap_or("");
    assert!(
        insert_text.contains("a :=") && insert_text.contains("b :="),
        "Snippet should contain parameter names: {insert_text}"
    );

    client.shutdown();
}

#[test]
fn test_document_symbols() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("symbols.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Add(a := 1, b := 2);\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected symbol array, got: {resp:?}");
    let symbols = result.as_array().unwrap();
    assert_eq!(symbols.len(), 2, "Expected 2 top-level symbols (Add + Main)");

    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"Add"), "Expected 'Add' symbol");
    assert!(names.contains(&"Main"), "Expected 'Main' symbol");

    let add_sym = symbols.iter().find(|s| s["name"].as_str() == Some("Add")).unwrap();
    let children = add_sym["children"].as_array().unwrap();
    assert!(children.len() >= 2, "Add should have at least 2 children (a, b)");

    client.shutdown();
}

// =============================================================================
// Signature Help
// =============================================================================

#[test]
fn test_signature_help() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("sighelp.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Add(a := 1, b := 2);\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Request signature help inside Add( call — position on 'Add'
    let resp = client.request(
        "textDocument/signatureHelp",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 12, "character": 17 }
        }),
    );

    let result = &resp["result"];
    if !result.is_null() {
        let sigs = result["signatures"].as_array().unwrap();
        assert!(!sigs.is_empty(), "Expected at least one signature");
        let sig_label = sigs[0]["label"].as_str().unwrap_or("");
        assert!(sig_label.contains("a:") || sig_label.contains("a :"), "Signature should contain param 'a': {sig_label}");
    }

    client.shutdown();
}

// =============================================================================
// References
// =============================================================================

#[test]
fn test_references() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("refs.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + 1;\n    counter := counter * 2;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 6 },
            "context": { "includeDeclaration": true }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected array of locations");
    let refs = result.as_array().unwrap();
    // 'counter' appears: declaration (line 2) + 4 usages (lines 4,4,5,5) = 5+
    assert!(refs.len() >= 3, "Expected at least 3 references to 'counter', got {}", refs.len());

    client.shutdown();
}

// =============================================================================
// Rename
// =============================================================================

#[test]
fn test_rename() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("rename.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    myVar : INT := 0;\nEND_VAR\n    myVar := myVar + 1;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 6 },
            "newName": "renamedVar"
        }),
    );

    let result = &resp["result"];
    assert!(!result.is_null(), "Expected workspace edit");
    let changes = &result["changes"];
    let edits = changes[uri.as_str()].as_array().unwrap();
    assert!(edits.len() >= 2, "Expected at least 2 edits (decl + usages), got {}", edits.len());

    // All edits should replace with "renamedVar"
    for edit in edits {
        assert_eq!(edit["newText"].as_str(), Some("renamedVar"));
    }

    client.shutdown();
}

// =============================================================================
// Formatting
// =============================================================================

#[test]
fn test_formatting() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("format.st");
    // Poorly indented code
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\nx : INT := 0;\nEND_VAR\nx := 1;\nIF x > 0 THEN\nx := 2;\nEND_IF;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected formatting edits");
    let edits = result.as_array().unwrap();
    assert!(!edits.is_empty(), "Expected at least one formatting edit");

    // The formatted text should have proper indentation
    let new_text = edits[0]["newText"].as_str().unwrap();
    assert!(new_text.contains("    x : INT"), "Expected indented variable: {new_text}");

    client.shutdown();
}

// =============================================================================
// Code Action (quick fix)
// =============================================================================

#[test]
fn test_code_action_declare_variable() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("codeaction.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared_var;\nEND_PROGRAM\n"
            }
        }),
    );
    let notif = client.wait_for_notification("textDocument/publishDiagnostics");
    let diags = notif["params"]["diagnostics"].as_array().unwrap();

    // Find the undeclared variable diagnostic
    let undeclared_diag = diags.iter().find(|d| {
        d["message"].as_str().unwrap_or("").contains("undeclared")
    });

    if let Some(diag) = undeclared_diag {
        let resp = client.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri },
                "range": diag["range"],
                "context": { "diagnostics": [diag] }
            }),
        );

        let result = &resp["result"];
        if !result.is_null() && result.is_array() {
            let actions = result.as_array().unwrap();
            assert!(!actions.is_empty(), "Expected at least one code action");
            let action = &actions[0];
            assert!(action["title"].as_str().unwrap_or("").contains("Declare"), "Expected declare action");
        }
    }

    client.shutdown();
}
