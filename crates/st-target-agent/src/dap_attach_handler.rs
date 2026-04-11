//! In-process DAP handler for attach-to-running-engine debug sessions.
//!
//! Translates DAP protocol messages (Content-Length framed JSON over TCP)
//! to `DebugCommand`/`DebugResponse` on the channels provided by
//! `RuntimeManager::debug_attach()`. No subprocess is spawned — the
//! debugger controls the same VM that was already running.

use crate::server::AppState;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Handle a DAP attach session on a TCP stream.
///
/// This function blocks until the debug session ends (disconnect or error).
/// It should be spawned on a blocking thread (not a tokio task) because
/// the std::sync::mpsc channels from `debug_attach()` use blocking recv.
pub fn handle_dap_attach(
    stream: std::net::TcpStream,
    app_state: Arc<AppState>,
    source_dir: &Path,
) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    info!("DAP attach: session from {peer}");

    // Attach to the running engine
    let rt = tokio::runtime::Handle::current();
    let (cmd_tx, event_rx) = match rt.block_on(app_state.runtime_manager.debug_attach()) {
        Ok(channels) => channels,
        Err(e) => {
            error!("DAP attach: cannot attach to engine: {e}");
            // Send a DAP error response and close
            send_dap_error(&stream, 0, &format!("Cannot attach: {e}"));
            return;
        }
    };

    // Load source files for breakpoint resolution
    let source_files = load_source_files(source_dir);

    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            error!("DAP attach: cannot clone stream: {e}");
            return;
        }
    };
    let writer_stream = stream;

    // Read DAP messages from TCP and dispatch
    let mut reader = BufReader::new(reader_stream);
    let mut seq_counter: i64 = 1;
    let mut initialized = false;

    loop {
        // Read a DAP message from the TCP stream
        let msg = match read_dap_message(&mut reader) {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                info!("DAP attach: client disconnected (EOF)");
                break;
            }
            Err(e) => {
                warn!("DAP attach: read error: {e}");
                break;
            }
        };

        let req_seq = msg["seq"].as_i64().unwrap_or(0);
        let command = msg["command"].as_str().unwrap_or("");

        match command {
            "initialize" => {
                send_dap_response(&writer_stream, req_seq, "initialize", serde_json::json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsEvaluateForHovers": true,
                }));
                initialized = true;
            }

            "attach" => {
                send_dap_response(&writer_stream, req_seq, "attach", serde_json::json!(null));
                // Send Initialized event
                send_dap_event(&writer_stream, &mut seq_counter, "initialized", serde_json::json!({}));
                // Send Stopped(entry) so VS Code shows the debug UI
                send_dap_event(&writer_stream, &mut seq_counter, "stopped", serde_json::json!({
                    "reason": "attach",
                    "threadId": 1,
                    "allThreadsStopped": true,
                }));
                // Pause the engine so the user can set breakpoints
                let _ = cmd_tx.send(st_engine::DebugCommand::Pause);
            }

            "configurationDone" => {
                send_dap_response(&writer_stream, req_seq, "configurationDone", serde_json::json!(null));
            }

            "threads" => {
                send_dap_response(&writer_stream, req_seq, "threads", serde_json::json!({
                    "threads": [{ "id": 1, "name": "PLC Scan Cycle" }]
                }));
            }

            "setBreakpoints" => {
                let source_path = msg["arguments"]["source"]["path"]
                    .as_str().unwrap_or("").to_string();
                let bp_lines: Vec<u32> = msg["arguments"]["breakpoints"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|b| b["line"].as_u64().map(|l| l as u32)).collect())
                    .unwrap_or_default();

                // Load source content for breakpoint resolution
                let source_content = find_source_content(&source_files, &source_path);

                let _ = cmd_tx.send(st_engine::DebugCommand::SetBreakpoints {
                    source_path: source_path.clone(),
                    source: source_content,
                    lines: bp_lines.clone(),
                });

                // Wait for verification response
                let verified = match event_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(st_engine::DebugResponse::BreakpointsSet { verified }) => verified,
                    _ => bp_lines.iter().map(|_| false).collect(),
                };

                let breakpoints: Vec<serde_json::Value> = bp_lines.iter().zip(verified.iter())
                    .enumerate()
                    .map(|(i, (line, ok))| serde_json::json!({
                        "id": i + 1,
                        "verified": ok,
                        "line": line,
                    }))
                    .collect();

                send_dap_response(&writer_stream, req_seq, "setBreakpoints", serde_json::json!({
                    "breakpoints": breakpoints,
                }));
            }

            "continue" => {
                send_dap_response(&writer_stream, req_seq, "continue", serde_json::json!({
                    "allThreadsContinued": true,
                }));
                let _ = cmd_tx.send(st_engine::DebugCommand::Continue);

                // Wait for the next Stopped event (breakpoint, step, pause)
                wait_for_stopped(&event_rx, &writer_stream, &mut seq_counter);
            }

            "next" => {
                send_dap_response(&writer_stream, req_seq, "next", serde_json::json!(null));
                let _ = cmd_tx.send(st_engine::DebugCommand::StepOver);
                wait_for_stopped(&event_rx, &writer_stream, &mut seq_counter);
            }

            "stepIn" => {
                send_dap_response(&writer_stream, req_seq, "stepIn", serde_json::json!(null));
                let _ = cmd_tx.send(st_engine::DebugCommand::StepIn);
                wait_for_stopped(&event_rx, &writer_stream, &mut seq_counter);
            }

            "stepOut" => {
                send_dap_response(&writer_stream, req_seq, "stepOut", serde_json::json!(null));
                let _ = cmd_tx.send(st_engine::DebugCommand::StepOut);
                wait_for_stopped(&event_rx, &writer_stream, &mut seq_counter);
            }

            "pause" => {
                let _ = cmd_tx.send(st_engine::DebugCommand::Pause);
                send_dap_response(&writer_stream, req_seq, "pause", serde_json::json!(null));
                wait_for_stopped(&event_rx, &writer_stream, &mut seq_counter);
            }

            "stackTrace" => {
                let _ = cmd_tx.send(st_engine::DebugCommand::GetStackTrace);
                match event_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(st_engine::DebugResponse::StackTrace { frames }) => {
                        let stack_frames: Vec<serde_json::Value> = frames.iter().enumerate()
                            .map(|(i, f)| {
                                // Convert source offset to line number
                                let (line, source_path) = resolve_frame_location(f, &source_files);
                                serde_json::json!({
                                    "id": i,
                                    "name": f.func_name,
                                    "source": {
                                        "name": std::path::Path::new(&source_path)
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                        "path": source_path,
                                    },
                                    "line": line,
                                    "column": 1,
                                })
                            })
                            .collect();
                        send_dap_response(&writer_stream, req_seq, "stackTrace", serde_json::json!({
                            "stackFrames": stack_frames,
                        }));
                    }
                    _ => {
                        send_dap_response(&writer_stream, req_seq, "stackTrace", serde_json::json!({
                            "stackFrames": [],
                        }));
                    }
                }
            }

            "scopes" => {
                send_dap_response(&writer_stream, req_seq, "scopes", serde_json::json!({
                    "scopes": [
                        { "name": "Locals", "variablesReference": 1000, "presentationHint": "locals" },
                        { "name": "Globals", "variablesReference": 1001, "presentationHint": "registers" },
                    ]
                }));
            }

            "variables" => {
                let var_ref = msg["arguments"]["variablesReference"].as_i64().unwrap_or(0);
                let scope = if var_ref == 1001 {
                    st_engine::DebugScopeKind::Globals
                } else {
                    st_engine::DebugScopeKind::Locals
                };
                let _ = cmd_tx.send(st_engine::DebugCommand::GetVariables { scope });
                match event_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(st_engine::DebugResponse::Variables { vars }) => {
                        let variables: Vec<serde_json::Value> = vars.iter()
                            .map(|v| serde_json::json!({
                                "name": v.name,
                                "value": v.value,
                                "type": v.ty,
                                "variablesReference": 0,
                            }))
                            .collect();
                        send_dap_response(&writer_stream, req_seq, "variables", serde_json::json!({
                            "variables": variables,
                        }));
                    }
                    _ => {
                        send_dap_response(&writer_stream, req_seq, "variables", serde_json::json!({
                            "variables": [],
                        }));
                    }
                }
            }

            "evaluate" => {
                let expr = msg["arguments"]["expression"].as_str().unwrap_or("");
                let _ = cmd_tx.send(st_engine::DebugCommand::Evaluate {
                    expression: expr.to_string(),
                });
                match event_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(st_engine::DebugResponse::EvaluateResult { value, ty }) => {
                        send_dap_response(&writer_stream, req_seq, "evaluate", serde_json::json!({
                            "result": value,
                            "type": ty,
                            "variablesReference": 0,
                        }));
                    }
                    _ => {
                        send_dap_response(&writer_stream, req_seq, "evaluate", serde_json::json!({
                            "result": "<unknown>",
                            "variablesReference": 0,
                        }));
                    }
                }
            }

            "disconnect" => {
                let _ = cmd_tx.send(st_engine::DebugCommand::Disconnect);
                send_dap_response(&writer_stream, req_seq, "disconnect", serde_json::json!(null));
                info!("DAP attach: disconnect requested");
                break;
            }

            other => {
                // Unknown command — respond with success (no-op) to keep VS Code happy
                if initialized {
                    send_dap_response(&writer_stream, req_seq, other, serde_json::json!(null));
                }
            }
        }
    }

    // Ensure clean detach
    let _ = cmd_tx.send(st_engine::DebugCommand::Disconnect);
    info!("DAP attach: session ended for {peer}");
}

// =============================================================================
// DAP message I/O helpers
// =============================================================================

fn read_dap_message(reader: &mut BufReader<std::net::TcpStream>) -> Result<Option<serde_json::Value>, String> {
    // Read Content-Length header
    let mut header = String::new();
    loop {
        header.clear();
        match reader.read_line(&mut header) {
            Ok(0) => return Ok(None), // EOF
            Ok(_) => {}
            Err(e) => return Err(format!("Read error: {e}")),
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break; // End of headers
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            let content_length: usize = len_str.trim().parse()
                .map_err(|_| "Invalid Content-Length".to_string())?;
            // Read the empty line after headers
            let mut blank = String::new();
            let _ = reader.read_line(&mut blank);
            // Read the body
            let mut body = vec![0u8; content_length];
            std::io::Read::read_exact(reader, &mut body)
                .map_err(|e| format!("Body read error: {e}"))?;
            let json: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|e| format!("JSON parse error: {e}"))?;
            return Ok(Some(json));
        }
    }
    Ok(None)
}

fn send_dap_message(stream: &std::net::TcpStream, json: &serde_json::Value) {
    let body = serde_json::to_string(json).unwrap_or_default();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut s = stream;
    let _ = s.write_all(header.as_bytes());
    let _ = s.write_all(body.as_bytes());
    let _ = s.flush();
}

fn send_dap_response(stream: &std::net::TcpStream, req_seq: i64, command: &str, body: serde_json::Value) {
    send_dap_message(stream, &serde_json::json!({
        "seq": 0,
        "type": "response",
        "request_seq": req_seq,
        "success": true,
        "command": command,
        "body": body,
    }));
}

fn send_dap_event(stream: &std::net::TcpStream, seq: &mut i64, event: &str, body: serde_json::Value) {
    *seq += 1;
    send_dap_message(stream, &serde_json::json!({
        "seq": *seq,
        "type": "event",
        "event": event,
        "body": body,
    }));
}

fn send_dap_error(stream: &std::net::TcpStream, req_seq: i64, message: &str) {
    send_dap_message(stream, &serde_json::json!({
        "seq": 0,
        "type": "response",
        "request_seq": req_seq,
        "success": false,
        "message": message,
    }));
}

// =============================================================================
// Source file helpers
// =============================================================================

/// Load all .st source files from the source directory.
fn load_source_files(source_dir: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    if source_dir.is_dir() {
        collect_st_files(source_dir, &mut files);
    }
    files
}

fn collect_st_files(dir: &Path, files: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_st_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "st" || e == "scl") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                files.push((path.to_string_lossy().to_string(), content));
            }
        }
    }
}

/// Find source content for a file path (tries exact match, then filename match).
fn find_source_content(source_files: &[(String, String)], path: &str) -> String {
    // Exact match
    if let Some((_, content)) = source_files.iter().find(|(p, _)| p == path) {
        return content.clone();
    }
    // Filename match (handles path remapping between local and target)
    let basename = Path::new(path).file_name().unwrap_or_default().to_string_lossy();
    if let Some((_, content)) = source_files.iter().find(|(p, _)| {
        Path::new(p).file_name().unwrap_or_default().to_string_lossy() == basename
    }) {
        return content.clone();
    }
    String::new()
}

/// Wait for a Stopped event from the engine and forward it to VS Code.
fn wait_for_stopped(
    event_rx: &std::sync::mpsc::Receiver<st_engine::DebugResponse>,
    writer: &std::net::TcpStream,
    seq: &mut i64,
) {
    // Wait up to 60 seconds for the engine to stop (it may run many cycles
    // before hitting a breakpoint).
    match event_rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(st_engine::DebugResponse::Stopped { reason }) => {
            let reason_str = match reason {
                st_engine::debug::PauseReason::Breakpoint => "breakpoint",
                st_engine::debug::PauseReason::Step => "step",
                st_engine::debug::PauseReason::PauseRequest => "pause",
                st_engine::debug::PauseReason::Entry => "entry",
                st_engine::debug::PauseReason::None => "pause",
            };
            send_dap_event(writer, seq, "stopped", serde_json::json!({
                "reason": reason_str,
                "threadId": 1,
                "allThreadsStopped": true,
            }));
        }
        Ok(st_engine::DebugResponse::Detached) => {
            send_dap_event(writer, seq, "terminated", serde_json::json!({}));
        }
        Ok(_) => {
            // Unexpected response — ignore
        }
        Err(_) => {
            // Timeout — engine didn't stop. This is fine for Continue with no breakpoints.
        }
    }
}

/// Resolve a stack frame's source offset to a line number and file path.
fn resolve_frame_location(
    frame: &st_engine::debug::FrameInfo,
    source_files: &[(String, String)],
) -> (u32, String) {
    // Find which source file this frame belongs to by matching function name
    // against declarations in source files (same heuristic as DapSession).
    for (path, content) in source_files {
        let upper = content.to_uppercase();
        // Check if this file declares the function
        for keyword in ["PROGRAM ", "FUNCTION_BLOCK ", "FUNCTION ", "CLASS "] {
            if let Some(idx) = upper.find(keyword) {
                let after = idx + keyword.len();
                let name_end = content[after..].find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(content.len() - after);
                let name = &content[after..after + name_end];
                if name.eq_ignore_ascii_case(&frame.func_name)
                    || frame.func_name.to_uppercase().starts_with(&name.to_uppercase())
                {
                    // Found the file — compute line from source offset
                    // The frame's source_offset is in virtual space. For now,
                    // use a simple byte-to-line conversion on this file's content.
                    let offset = frame.source_offset.min(content.len());
                    let line = content[..offset].matches('\n').count() as u32 + 1;
                    return (line, path.clone());
                }
            }
        }
    }
    // Fallback: first source file
    if let Some((path, content)) = source_files.first() {
        let offset = frame.source_offset.min(content.len());
        let line = content[..offset].matches('\n').count() as u32 + 1;
        return (line, path.clone());
    }
    (1, String::new())
}
