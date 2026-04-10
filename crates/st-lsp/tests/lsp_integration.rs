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

// =============================================================================
// Document Highlight
// =============================================================================

#[test]
fn test_document_highlight() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("highlight.st");
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
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/documentHighlight",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 6 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected highlight array");
    let highlights = result.as_array().unwrap();
    assert!(highlights.len() >= 3, "Expected at least 3 highlights for 'counter', got {}", highlights.len());
}

// =============================================================================
// Folding Ranges
// =============================================================================

#[test]
fn test_folding_ranges() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("folding.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n    END_IF;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": uri } }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected folding ranges array");
    let ranges = result.as_array().unwrap();
    assert!(ranges.len() >= 2, "Expected at least 2 folding ranges, got {}", ranges.len());
}

// =============================================================================
// Workspace Symbol
// =============================================================================

#[test]
fn test_workspace_symbol() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("wssymbol.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "FUNCTION Helper : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Helper := x;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    r : INT := 0;\nEND_VAR\n    r := Helper(x := 1);\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "workspace/symbol",
        json!({ "query": "Help" }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected symbol array");
    let symbols = result.as_array().unwrap();
    assert!(!symbols.is_empty(), "Expected at least one symbol matching 'Help'");
    assert!(symbols.iter().any(|s| s["name"].as_str() == Some("Helper")));

    // Empty query — should return all symbols
    let resp2 = client.request(
        "workspace/symbol",
        json!({ "query": "" }),
    );
    let all_symbols = resp2["result"].as_array().unwrap();
    assert!(all_symbols.len() >= 2, "Expected at least Main + Helper");

    client.shutdown();
}

// =============================================================================
// Selection Range (smart expand / shrink selection)
// =============================================================================

#[test]
fn test_selection_range_returns_nested_chain() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source with nested structure: PROGRAM > IF > assignment
    // Lines (0-indexed):
    //   0  PROGRAM Main
    //   1  VAR
    //   2      x : INT := 0;
    //   3  END_VAR
    //   4      IF x > 0 THEN
    //   5          x := 1;         <-- cursor here (line 5, char 10, inside "1")
    //   6      END_IF;
    //   7  END_PROGRAM
    let uri = file_uri("selrange.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n    END_IF;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/selectionRange",
        json!({
            "textDocument": { "uri": uri },
            "positions": [{ "line": 5, "character": 13 }]
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected array of SelectionRange, got: {result:?}");
    let ranges = result.as_array().unwrap();
    assert_eq!(ranges.len(), 1, "One range per position");

    // Walk the chain outward and collect each level's range
    let mut levels = Vec::new();
    let mut current = Some(&ranges[0]);
    while let Some(sr) = current {
        let r = &sr["range"];
        let start_line = r["start"]["line"].as_u64().unwrap();
        let end_line = r["end"]["line"].as_u64().unwrap();
        levels.push((start_line, end_line));
        current = sr.get("parent").filter(|p| !p.is_null());
    }

    // We expect at least 3 levels: word/expression, statement, PROGRAM
    assert!(
        levels.len() >= 3,
        "Expected at least 3 nesting levels (word → statement → PROGRAM), got {} levels: {:?}",
        levels.len(),
        levels
    );

    // Innermost should be on line 5 (the expression/statement)
    assert_eq!(levels[0].0, 5, "Innermost range should start on line 5");

    // Outermost should span the whole PROGRAM (lines 0-7)
    let outermost = levels.last().unwrap();
    assert_eq!(outermost.0, 0, "Outermost range should start at line 0 (PROGRAM)");
    assert!(
        outermost.1 >= 7,
        "Outermost range should end at or past line 7 (END_PROGRAM)"
    );

    // Each level should be strictly larger than (or equal to) the previous
    for window in levels.windows(2) {
        let inner_span = window[0].1 - window[0].0;
        let outer_span = window[1].1 - window[1].0;
        assert!(
            outer_span >= inner_span,
            "Each parent range must be >= its child: child span {inner_span}, parent span {outer_span}"
        );
    }

    client.shutdown();
}

#[test]
fn test_selection_range_multiple_positions() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("selrange2.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": "PROGRAM Main\nVAR\n    x : INT := 0;\n    y : INT := 0;\nEND_VAR\n    x := 1;\n    y := 2;\nEND_PROGRAM\n"
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Two positions: one in the VAR block (line 2), one in the body (line 6)
    let resp = client.request(
        "textDocument/selectionRange",
        json!({
            "textDocument": { "uri": uri },
            "positions": [
                { "line": 2, "character": 4 },
                { "line": 6, "character": 4 }
            ]
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected array");
    let ranges = result.as_array().unwrap();
    assert_eq!(ranges.len(), 2, "Should return one SelectionRange per position");

    // Both should have a parent chain (at least 2 levels: current + PROGRAM)
    for (i, sr) in ranges.iter().enumerate() {
        assert!(
            sr.get("parent").is_some() && !sr["parent"].is_null(),
            "Position {i}: expected at least one parent level"
        );
    }

    client.shutdown();
}

#[test]
fn test_selection_range_on_keyword() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("selrange3.st");
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
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "PROGRAM" keyword (line 0, char 3 = inside "GRAM")
    let resp = client.request(
        "textDocument/selectionRange",
        json!({
            "textDocument": { "uri": uri },
            "positions": [{ "line": 0, "character": 3 }]
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array());
    let ranges = result.as_array().unwrap();
    assert_eq!(ranges.len(), 1);

    // The innermost range should cover the word "PROGRAM" (line 0, chars 0-7)
    let inner = &ranges[0]["range"];
    assert_eq!(inner["start"]["line"].as_u64(), Some(0));
    assert_eq!(inner["start"]["character"].as_u64(), Some(0));
    assert_eq!(inner["end"]["character"].as_u64(), Some(7)); // "PROGRAM" = 7 chars

    client.shutdown();
}

#[test]
fn test_selection_range_capability_advertised() {
    let mut client = TestClient::start();
    let resp = client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );

    let caps = &resp["result"]["capabilities"];
    assert!(
        caps["selectionRangeProvider"].as_bool().unwrap_or(false),
        "selectionRangeProvider should be advertised: {caps:?}"
    );
    assert!(
        caps["inlayHintProvider"].as_bool().unwrap_or(false),
        "inlayHintProvider should be advertised: {caps:?}"
    );

    client.notify("initialized", json!({}));
    client.shutdown();
}

// =============================================================================
// Inlay Hints (parameter names at call sites)
// =============================================================================

#[test]
fn test_inlay_hint_parameter_names() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source: a function with two parameters, called with positional args.
    //   0  FUNCTION Add : INT
    //   1  VAR_INPUT
    //   2      a : INT;
    //   3      b : INT;
    //   4  END_VAR
    //   5      Add := a + b;
    //   6  END_FUNCTION
    //   7
    //   8  PROGRAM Main
    //   9  VAR
    //  10      result : INT := 0;
    //  11  END_VAR
    //  12      result := Add(10, 20);   <-- positional call, expect hints
    //  13  END_PROGRAM
    let uri = file_uri("inlayhint.st");
    let source = "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Add(10, 20);\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/inlayHint",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 13, "character": 0 }
            }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected inlay hints array, got: {result:?}");
    let hints = result.as_array().unwrap();

    // Should have 2 hints: "a:" before 10, "b:" before 20
    assert_eq!(
        hints.len(),
        2,
        "Expected 2 parameter hints (a: and b:), got {}: {hints:?}",
        hints.len()
    );

    // First hint: "a:" at position of "10"
    assert_eq!(hints[0]["label"].as_str(), Some("a:"));
    assert_eq!(hints[0]["kind"].as_u64(), Some(2)); // InlayHintKind::PARAMETER = 2
    assert_eq!(hints[0]["position"]["line"].as_u64(), Some(12));

    // Second hint: "b:" at position of "20"
    assert_eq!(hints[1]["label"].as_str(), Some("b:"));
    assert_eq!(hints[1]["position"]["line"].as_u64(), Some(12));

    client.shutdown();
}

#[test]
fn test_inlay_hint_skips_named_arguments() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Named arguments already show the parameter name — no hint needed.
    let uri = file_uri("inlayhint_named.st");
    let source = "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Add(a := 10, b := 20);\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/inlayHint",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 13, "character": 0 }
            }
        }),
    );

    // Named args → no hints should be generated
    let result = &resp["result"];
    assert!(
        result.is_null() || result.as_array().map_or(false, |a| a.is_empty()),
        "Expected no hints for named arguments, got: {result:?}"
    );

    client.shutdown();
}

#[test]
fn test_inlay_hint_skips_when_arg_matches_param_name() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // When the argument text matches the parameter name, the hint is
    // redundant and should be suppressed (e.g., `Add(a, b)` where the
    // params are also named a and b).
    let uri = file_uri("inlayhint_match.st");
    let source = "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    a : INT := 10;\n    b : INT := 20;\nEND_VAR\n    a := Add(a, b);\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/inlayHint",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 14, "character": 0 }
            }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_null() || result.as_array().map_or(false, |a| a.is_empty()),
        "Expected no hints when arg names match param names, got: {result:?}"
    );

    client.shutdown();
}

// =============================================================================
// Call Hierarchy (cross-reference: who calls what)
// =============================================================================

/// Source with a call chain: Main → Helper → Validate
const CALL_HIERARCHY_SOURCE: &str = "\
FUNCTION Validate : BOOL\n\
VAR_INPUT\n\
    val : INT;\n\
END_VAR\n\
    Validate := val > 0;\n\
END_FUNCTION\n\
\n\
FUNCTION Helper : INT\n\
VAR_INPUT\n\
    x : INT;\n\
END_VAR\n\
    IF Validate(val := x) THEN\n\
        Helper := x * 2;\n\
    ELSE\n\
        Helper := 0;\n\
    END_IF;\n\
END_FUNCTION\n\
\n\
PROGRAM Main\n\
VAR\n\
    a : INT := 0;\n\
    b : INT := 0;\n\
END_VAR\n\
    a := Helper(x := 10);\n\
    b := Helper(x := 20);\n\
END_PROGRAM\n";

#[test]
fn test_prepare_call_hierarchy_on_function() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("callhier.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": CALL_HIERARCHY_SOURCE
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "Helper" function declaration (line 7 = FUNCTION Helper)
    let resp = client.request(
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 7, "character": 12 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected array, got: {result:?}");
    let items = result.as_array().unwrap();
    assert_eq!(items.len(), 1, "Expected one CallHierarchyItem");
    assert_eq!(
        items[0]["name"].as_str().unwrap().to_uppercase(),
        "HELPER",
        "Expected item named Helper"
    );
    assert!(items[0]["kind"].is_number(), "Expected a symbol kind");

    client.shutdown();
}

#[test]
fn test_incoming_calls_finds_callers() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("callhier_in.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": CALL_HIERARCHY_SOURCE
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // First, prepare on "Helper" to get its CallHierarchyItem
    let prep = client.request(
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 7, "character": 12 }
        }),
    );
    let item = &prep["result"][0];

    // Then ask for incoming calls (who calls Helper?)
    let resp = client.request(
        "callHierarchy/incomingCalls",
        json!({ "item": item }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected incoming calls array, got: {result:?}");
    let calls = result.as_array().unwrap();

    // Helper is called by Main (twice: lines 23 and 24)
    assert!(
        !calls.is_empty(),
        "Expected at least one caller of Helper"
    );
    let caller_names: Vec<&str> = calls
        .iter()
        .filter_map(|c| c["from"]["name"].as_str())
        .collect();
    assert!(
        caller_names.iter().any(|n| n.eq_ignore_ascii_case("Main")),
        "Expected Main to call Helper, got callers: {caller_names:?}"
    );

    // The from_ranges should have 2 entries (two calls on lines 23 and 24)
    let main_call = calls
        .iter()
        .find(|c| {
            c["from"]["name"]
                .as_str()
                .map_or(false, |n| n.eq_ignore_ascii_case("Main"))
        })
        .unwrap();
    let from_ranges = main_call["fromRanges"].as_array().unwrap();
    assert_eq!(
        from_ranges.len(),
        2,
        "Main calls Helper twice, expected 2 ranges, got {}",
        from_ranges.len()
    );

    client.shutdown();
}

#[test]
fn test_outgoing_calls_finds_callees() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("callhier_out.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": CALL_HIERARCHY_SOURCE
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Prepare on "Helper"
    let prep = client.request(
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 7, "character": 12 }
        }),
    );
    let item = &prep["result"][0];

    // Ask for outgoing calls (what does Helper call?)
    let resp = client.request(
        "callHierarchy/outgoingCalls",
        json!({ "item": item }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected outgoing calls array, got: {result:?}");
    let calls = result.as_array().unwrap();

    // Helper calls Validate (once, inside the IF)
    let callee_names: Vec<&str> = calls
        .iter()
        .filter_map(|c| c["to"]["name"].as_str())
        .collect();
    assert!(
        callee_names.iter().any(|n| n.eq_ignore_ascii_case("Validate")),
        "Expected Helper to call Validate, got callees: {callee_names:?}"
    );

    client.shutdown();
}

#[test]
fn test_incoming_calls_of_validate_finds_helper() {
    // Validate is called only by Helper (not by Main directly).
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("callhier_val.st");
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": CALL_HIERARCHY_SOURCE
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Prepare on "Validate" (line 0 = FUNCTION Validate)
    let prep = client.request(
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 12 }
        }),
    );
    let item = &prep["result"][0];

    let resp = client.request(
        "callHierarchy/incomingCalls",
        json!({ "item": item }),
    );

    let calls = resp["result"].as_array().unwrap();
    let caller_names: Vec<&str> = calls
        .iter()
        .filter_map(|c| c["from"]["name"].as_str())
        .collect();
    assert!(
        caller_names.iter().any(|n| n.eq_ignore_ascii_case("Helper")),
        "Expected Helper to call Validate, got callers: {caller_names:?}"
    );
    // Main does NOT call Validate directly
    assert!(
        !caller_names.iter().any(|n| n.eq_ignore_ascii_case("Main")),
        "Main should NOT appear as a direct caller of Validate"
    );

    client.shutdown();
}

#[test]
fn test_call_hierarchy_capability_advertised() {
    let mut client = TestClient::start();
    let resp = client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );

    let caps = &resp["result"]["capabilities"];
    assert!(
        caps["callHierarchyProvider"].as_bool().unwrap_or(false),
        "callHierarchyProvider should be advertised: {caps:?}"
    );
    assert!(
        caps["documentOnTypeFormattingProvider"].is_object(),
        "documentOnTypeFormattingProvider should be advertised: {caps:?}"
    );

    client.notify("initialized", json!({}));
    client.shutdown();
}

// =============================================================================
// On-Type Formatting (auto-indent after Enter / ;)
// =============================================================================

#[test]
fn test_on_type_formatting_indent_after_if_then() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source BEFORE the user presses Enter after THEN:
    //   0  PROGRAM Main
    //   1  VAR
    //   2      x : INT := 0;
    //   3  END_VAR
    //   4      IF x > 0 THEN
    //   5  <cursor here after Enter — new empty line>
    //   6      END_IF;
    //   7  END_PROGRAM
    let uri = file_uri("ontypefmt_then.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n\n    END_IF;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Simulate Enter at line 5 (the empty line after THEN)
    let resp = client.request(
        "textDocument/onTypeFormatting",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 5, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_array(),
        "Expected TextEdit array for auto-indent after THEN, got: {result:?}"
    );
    let edits = result.as_array().unwrap();
    assert!(!edits.is_empty(), "Expected at least one indent edit");

    // The edit should set the new line's indent to 8 spaces (4 for IF scope + 4 for THEN body)
    let new_text = edits[0]["newText"].as_str().unwrap_or("");
    assert_eq!(
        new_text.len(),
        8,
        "Expected 8 spaces of indent (2 levels × 4), got {} spaces: {:?}",
        new_text.len(),
        new_text
    );

    client.shutdown();
}

#[test]
fn test_on_type_formatting_indent_after_var() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source with cursor on empty line after VAR
    //   0  PROGRAM Main
    //   1  VAR
    //   2  <cursor — empty line after VAR>
    //   3  END_VAR
    //   4      x := 1;
    //   5  END_PROGRAM
    let uri = file_uri("ontypefmt_var.st");
    let source = "PROGRAM Main\nVAR\n\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/onTypeFormatting",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 2, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_array(), "Expected edits, got: {result:?}");
    let edits = result.as_array().unwrap();
    assert!(!edits.is_empty());

    // VAR at indent 0 → body should be at indent 4
    let new_text = edits[0]["newText"].as_str().unwrap_or("");
    assert_eq!(
        new_text.len(),
        4,
        "Expected 4 spaces indent after VAR, got {}: {:?}",
        new_text.len(),
        new_text
    );

    client.shutdown();
}

#[test]
fn test_on_type_formatting_no_indent_change_for_normal_line() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source where previous line is a normal statement (no opener)
    //   0  PROGRAM Main
    //   1  VAR
    //   2      x : INT := 0;
    //   3  END_VAR
    //   4      x := 1;
    //   5  <cursor — should match indent of line 4>
    //   6  END_PROGRAM
    let uri = file_uri("ontypefmt_normal.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\n\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    let resp = client.request(
        "textDocument/onTypeFormatting",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 5, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    );

    let result = &resp["result"];
    // Either null (no change needed) or an edit that sets 4-space indent
    // (matching the previous statement's indent level).
    if result.is_array() {
        let edits = result.as_array().unwrap();
        if !edits.is_empty() {
            let new_text = edits[0]["newText"].as_str().unwrap_or("");
            assert_eq!(
                new_text.len(),
                4,
                "Normal line should match previous indent (4 spaces), got {}",
                new_text.len()
            );
        }
    }
    // null is also acceptable (previous indent matches, no edit needed)

    client.shutdown();
}

#[test]
fn test_on_type_formatting_semicolon_reindents_end_if() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source where END_IF; is at the WRONG indent level (too deep).
    // After typing ';', the formatter should reindent it.
    //   0  PROGRAM Main
    //   1  VAR
    //   2      x : INT := 0;
    //   3  END_VAR
    //   4      IF x > 0 THEN
    //   5          x := 1;
    //   6          END_IF;   <-- too deep, should be 4 spaces
    //   7  END_PROGRAM
    let uri = file_uri("ontypefmt_semi.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n        END_IF;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Simulate ';' typed at the end of "        END_IF;" (line 6)
    let resp = client.request(
        "textDocument/onTypeFormatting",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 6, "character": 14 },
            "ch": ";",
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_array(),
        "Expected TextEdit for END_IF reindent, got: {result:?}"
    );
    let edits = result.as_array().unwrap();
    assert!(!edits.is_empty(), "Expected a reindent edit");

    // The edit should reduce indent from 8 to 4 spaces.
    let new_text = edits[0]["newText"].as_str().unwrap_or("");
    assert_eq!(
        new_text.len(),
        4,
        "END_IF should be reindented to 4 spaces, got {}: {:?}",
        new_text.len(),
        new_text
    );

    client.shutdown();
}

// =============================================================================
// Linked Editing Range (matching keyword pairs: IF ↔ END_IF, etc.)
// =============================================================================

#[test]
fn test_linked_editing_range_if_end_if() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    // Source:
    //   0  PROGRAM Main
    //   1  VAR
    //   2      x : INT := 0;
    //   3  END_VAR
    //   4      IF x > 0 THEN
    //   5          x := 1;
    //   6      END_IF;
    //   7  END_PROGRAM
    let uri = file_uri("linked_if.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n    END_IF;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "IF" keyword (line 4, character 4 = inside "IF")
    let resp = client.request(
        "textDocument/linkedEditingRange",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 5 }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_object(),
        "Expected LinkedEditingRanges object, got: {result:?}"
    );
    let ranges = result["ranges"].as_array().unwrap();
    assert_eq!(
        ranges.len(),
        2,
        "Expected 2 linked ranges (IF + END_IF), got {}: {ranges:?}",
        ranges.len()
    );

    // First range should be "IF" (line 4)
    assert_eq!(ranges[0]["start"]["line"].as_u64(), Some(4));
    // Second range should be "END_IF" (line 6)
    assert_eq!(ranges[1]["start"]["line"].as_u64(), Some(6));

    client.shutdown();
}

#[test]
fn test_linked_editing_range_from_end_keyword() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("linked_end.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n    END_IF;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "END_IF" keyword (line 6, inside the word)
    let resp = client.request(
        "textDocument/linkedEditingRange",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 6, "character": 6 }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_object(),
        "Expected linked ranges from END_IF cursor, got: {result:?}"
    );
    let ranges = result["ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 2);

    // Should link END_IF back to IF
    let lines: Vec<u64> = ranges
        .iter()
        .filter_map(|r| r["start"]["line"].as_u64())
        .collect();
    assert!(lines.contains(&4), "Expected IF on line 4: {lines:?}");
    assert!(lines.contains(&6), "Expected END_IF on line 6: {lines:?}");

    client.shutdown();
}

#[test]
fn test_linked_editing_range_program_end_program() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("linked_prog.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "PROGRAM" (line 0, character 3)
    let resp = client.request(
        "textDocument/linkedEditingRange",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 3 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected linked ranges, got: {result:?}");
    let ranges = result["ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0]["start"]["line"].as_u64(), Some(0)); // PROGRAM
    assert_eq!(ranges[1]["start"]["line"].as_u64(), Some(5)); // END_PROGRAM

    client.shutdown();
}

#[test]
fn test_linked_editing_range_var_end_var() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("linked_var.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "VAR" (line 1)
    let resp = client.request(
        "textDocument/linkedEditingRange",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 1 }
        }),
    );

    let result = &resp["result"];
    assert!(result.is_object(), "Expected linked ranges for VAR, got: {result:?}");
    let ranges = result["ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0]["start"]["line"].as_u64(), Some(1)); // VAR
    assert_eq!(ranges[1]["start"]["line"].as_u64(), Some(3)); // END_VAR

    client.shutdown();
}

#[test]
fn test_linked_editing_range_no_result_on_non_keyword() {
    let mut client = TestClient::start();
    client.request(
        "initialize",
        json!({ "processId": null, "capabilities": {}, "rootUri": "file:///test" }),
    );
    client.notify("initialized", json!({}));

    let uri = file_uri("linked_none.st");
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "structured-text",
                "version": 1,
                "text": source
            }
        }),
    );
    client.wait_for_notification("textDocument/publishDiagnostics");

    // Cursor on "x" (a variable, not a keyword)
    let resp = client.request(
        "textDocument/linkedEditingRange",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 5 }
        }),
    );

    let result = &resp["result"];
    assert!(
        result.is_null(),
        "Expected null for non-keyword position, got: {result:?}"
    );

    client.shutdown();
}
