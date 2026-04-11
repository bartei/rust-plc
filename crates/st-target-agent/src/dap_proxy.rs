//! DAP (Debug Adapter Protocol) TCP proxy.
//!
//! Accepts TCP connections from VS Code, spawns `st-cli debug <source_path>`
//! as a subprocess, and bidirectionally bridges the TCP stream with the
//! subprocess's stdio. Both sides use Content-Length framing, so the bridge
//! is a simple byte copy.
//!
//! The proxy checks the bundle mode before allowing connections:
//! - **Development**: full debug allowed
//! - **Release-debug**: limited debug allowed (obfuscated names)
//! - **Release**: connection rejected (no debug info)

use crate::server::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use tracing::{error, info, warn};

/// Run the DAP proxy TCP server.
///
/// Listens on `bind:port` and accepts one connection at a time. Each connection
/// spawns a `st-cli debug` subprocess and bridges the TCP stream with its stdio.
pub async fn run_dap_proxy(
    bind: String,
    port: u16,
    app_state: Arc<AppState>,
    st_cli_path: PathBuf,
) {
    let addr = format!("{bind}:{port}");
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("DAP proxy: cannot bind to {addr}: {e}");
            return;
        }
    };
    run_dap_proxy_with_listener(listener, app_state, st_cli_path).await;
}

/// Run the DAP proxy with a pre-bound listener (for testing).
pub async fn run_dap_proxy_with_listener(
    listener: TcpListener,
    app_state: Arc<AppState>,
    st_cli_path: PathBuf,
) {
    info!("DAP proxy listening on {:?}", listener.local_addr());

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                error!("DAP proxy: accept error: {e}");
                continue;
            }
        };

        info!("DAP proxy: connection from {peer}");

        let source_path = {
            let store = app_state.program_store.read().unwrap();
            match store.current_program() {
                None => {
                    warn!("DAP proxy: no program deployed, rejecting connection");
                    drop(stream);
                    continue;
                }
                Some(meta) => {
                    if meta.mode == "release" {
                        warn!("DAP proxy: program is in release mode (no debug info), rejecting");
                        drop(stream);
                        continue;
                    }
                    // The source path for DAP is the project directory or first source file
                    // For now, use the program store's source directory
                    store.source_path().unwrap_or_default()
                }
            }
        };

        if source_path.as_os_str().is_empty() {
            warn!("DAP proxy: no source path available for debugging");
            drop(stream);
            continue;
        }

        // Route based on engine state:
        // - Running/DebugPaused → attach to running engine (no subprocess)
        // - Idle → spawn subprocess for offline debugging
        let status = app_state.runtime_manager.state().status;
        info!("DAP proxy: engine status = {status:?}");
        if status == crate::runtime_manager::RuntimeStatus::Running
            || status == crate::runtime_manager::RuntimeStatus::DebugPaused
        {
            info!("DAP proxy: attaching to running engine (no subprocess)");
            let state_clone = Arc::clone(&app_state);
            let src = source_path.clone();
            // Convert tokio TcpStream to std TcpStream for the blocking handler
            let std_stream = stream.into_std().unwrap();
            // Spawn on a blocking thread (handler uses std::sync::mpsc blocking recv)
            tokio::task::spawn_blocking(move || {
                crate::dap_attach_handler::handle_dap_attach(
                    std_stream, state_clone, &src,
                );
                info!("DAP proxy: attach session ended for {peer}");
            });
        } else {
            let cli_path = st_cli_path.clone();
            // Offline debug: spawn subprocess
            tokio::spawn(async move {
                if let Err(e) = handle_dap_connection(stream, &cli_path, &source_path).await {
                    error!("DAP proxy session error: {e}");
                }
                info!("DAP proxy: session ended for {peer}");
            });
        }
    }
}

/// Handle a single DAP connection by bridging TCP ↔ subprocess stdio.
async fn handle_dap_connection(
    stream: tokio::net::TcpStream,
    st_cli_path: &std::path::Path,
    source_path: &std::path::Path,
) -> Result<(), String> {
    // Spawn st-cli debug as a subprocess (using tokio::process for async I/O)
    info!("DAP proxy: spawning st-cli debug {}", source_path.display());
    let mut child = Command::new(st_cli_path)
        .args(["debug", &source_path.to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Cannot spawn st-cli debug: {e}"))?;
    info!("DAP proxy: subprocess PID {:?}", child.id());

    let child_stdin = child.stdin.take().ok_or("No stdin on child")?;
    let child_stdout = child.stdout.take().ok_or("No stdout on child")?;

    let (tcp_reader, tcp_writer) = stream.into_split();

    // Bridge: TCP → subprocess stdin
    let tcp_to_stdin = tokio::spawn(copy_stream(tcp_reader, child_stdin, "tcp→stdin"));

    // Bridge: subprocess stdout → TCP
    let stdout_to_tcp = tokio::spawn(copy_stream(child_stdout, tcp_writer, "stdout→tcp"));

    // Wait for either direction to finish
    tokio::select! {
        r = tcp_to_stdin => {
            if let Err(e) = r {
                warn!("DAP proxy tcp→stdin task error: {e}");
            }
        }
        r = stdout_to_tcp => {
            if let Err(e) = r {
                warn!("DAP proxy stdout→tcp task error: {e}");
            }
        }
    }

    // Kill the subprocess
    let _ = child.kill().await;
    let _ = child.wait().await;

    Ok(())
}

/// Copy bytes from reader to writer until EOF.
async fn copy_stream<R, W>(mut reader: R, mut writer: W, _label: &str) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
    
            break;
        }
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
    }
    Ok(())
}
