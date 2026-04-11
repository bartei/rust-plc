//! Runtime watchdog: crash detection and auto-restart.

use crate::config::RuntimeConfig;
use crate::runtime_manager::{RuntimeCommand, RuntimeState, RuntimeStatus};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{error, info, warn};

/// Monitors the runtime thread and auto-restarts on crash.
pub struct Watchdog {
    state: Arc<RwLock<RuntimeState>>,
    config: RuntimeConfig,
    #[allow(dead_code)] // Will be used when full restart logic is wired
    cmd_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
}

impl Watchdog {
    pub fn new(
        state: Arc<RwLock<RuntimeState>>,
        config: RuntimeConfig,
        cmd_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
    ) -> Self {
        Watchdog {
            state,
            config,
            cmd_tx,
        }
    }

    /// Run the watchdog loop. This should be spawned as a tokio task.
    pub async fn run(self) {
        let poll_interval = self
            .config
            .watchdog_ms
            .map(|ms| Duration::from_millis(ms.min(1000)))
            .unwrap_or(Duration::from_secs(1));

        let mut last_cycle_count: u64 = 0;
        let mut healthy_since: Option<tokio::time::Instant> = None;

        loop {
            tokio::time::sleep(poll_interval).await;

            let state = self.state.read().unwrap().clone();

            match state.status {
                RuntimeStatus::Running => {
                    let current_count = state
                        .cycle_stats
                        .as_ref()
                        .map(|s| s.cycle_count)
                        .unwrap_or(0);

                    if current_count > last_cycle_count {
                        last_cycle_count = current_count;
                        if healthy_since.is_none() {
                            healthy_since = Some(tokio::time::Instant::now());
                        }
                    }

                    // Reset restart count after sustained healthy run (60s)
                    if let Some(since) = healthy_since {
                        if since.elapsed() > Duration::from_secs(60) && state.restart_count > 0 {
                            info!("Runtime healthy for 60s, resetting restart counter");
                            let mut s = self.state.write().unwrap();
                            s.restart_count = 0;
                            healthy_since = Some(tokio::time::Instant::now());
                        }
                    }
                }

                RuntimeStatus::Error if self.config.restart_on_crash => {
                    healthy_since = None;
                    last_cycle_count = 0;

                    let restart_count = state.restart_count;
                    if restart_count >= self.config.max_restarts {
                        error!(
                            "Max restarts ({}) exceeded, giving up",
                            self.config.max_restarts
                        );
                        continue;
                    }

                    warn!(
                        "Runtime crashed (restart {}/{}), restarting in {}ms",
                        restart_count + 1,
                        self.config.max_restarts,
                        self.config.restart_delay_ms,
                    );

                    tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;

                    // Increment restart counter
                    {
                        let mut s = self.state.write().unwrap();
                        s.restart_count += 1;
                    }

                    // The actual restart requires the module and program name,
                    // which we don't have here. The API layer handles re-starting
                    // by re-loading from the program store. The watchdog sets the
                    // state to signal that a restart is needed.
                    // For now, we log the intent. Full restart logic will be
                    // wired when the API layer detects Error + restart_count < max.
                    info!("Watchdog: restart signaled (restart count: {})", restart_count + 1);
                }

                RuntimeStatus::Idle => {
                    healthy_since = None;
                    last_cycle_count = 0;
                }

                RuntimeStatus::DebugPaused => {
                    // Engine is paused by an attached debugger — this is expected.
                    // Do NOT count as a stall. Do NOT restart.
                }

                _ => {}
            }
        }
    }
}
