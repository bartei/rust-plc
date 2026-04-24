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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_manager::{CycleStatsSnapshot, RuntimeState};

    fn make_state(status: RuntimeStatus) -> Arc<RwLock<RuntimeState>> {
        Arc::new(RwLock::new(RuntimeState {
            status,
            ..RuntimeState::default()
        }))
    }

    fn make_watchdog(
        state: Arc<RwLock<RuntimeState>>,
        config: RuntimeConfig,
    ) -> (Watchdog, tokio::sync::mpsc::Receiver<RuntimeCommand>) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);
        (Watchdog::new(state, config, cmd_tx), cmd_rx)
    }

    fn bump_cycles(state: &Arc<RwLock<RuntimeState>>, count: u64) {
        let mut s = state.write().unwrap();
        s.cycle_stats = Some(CycleStatsSnapshot {
            cycle_count: count,
            ..CycleStatsSnapshot::default()
        });
    }

    /// Pump the watchdog loop N times: advance the virtual clock by `step`
    /// each tick and yield so the task can observe the new state.
    async fn pump(ticks: u32, step: Duration) {
        for _ in 0..ticks {
            tokio::time::advance(step).await;
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn idle_status_is_noop() {
        let state = make_state(RuntimeStatus::Idle);
        let config = RuntimeConfig {
            watchdog_ms: Some(50),
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        pump(4, Duration::from_millis(60)).await;
        // Nothing should have changed.
        let s = state.read().unwrap();
        assert_eq!(s.restart_count, 0);
        assert_eq!(s.status, RuntimeStatus::Idle);
        drop(s);
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn debug_paused_does_not_count_as_stall() {
        let state = make_state(RuntimeStatus::DebugPaused);
        // Deliberately do NOT bump cycle_count — a stalled engine in DebugPaused
        // must not trigger restart logic.
        let config = RuntimeConfig {
            watchdog_ms: Some(50),
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        pump(10, Duration::from_millis(60)).await;
        assert_eq!(state.read().unwrap().restart_count, 0);
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn error_without_restart_on_crash_does_nothing() {
        let state = make_state(RuntimeStatus::Error);
        let config = RuntimeConfig {
            watchdog_ms: Some(50),
            restart_on_crash: false,
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        pump(30, Duration::from_millis(60)).await;
        assert_eq!(state.read().unwrap().restart_count, 0);
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn error_status_increments_restart_count() {
        let state = make_state(RuntimeStatus::Error);
        let config = RuntimeConfig {
            watchdog_ms: Some(50),
            restart_on_crash: true,
            restart_delay_ms: 100,
            max_restarts: 5,
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        // One watchdog cycle (50 ms poll + 100 ms restart sleep) = ~150 ms.
        pump(10, Duration::from_millis(60)).await;
        let count = state.read().unwrap().restart_count;
        assert!(count >= 1, "restart_count should have incremented, got {count}");
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn error_status_stops_at_max_restarts() {
        let state = make_state(RuntimeStatus::Error);
        state.write().unwrap().restart_count = 3;
        let config = RuntimeConfig {
            watchdog_ms: Some(50),
            restart_on_crash: true,
            restart_delay_ms: 100,
            max_restarts: 3,
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        pump(30, Duration::from_millis(60)).await;
        assert_eq!(
            state.read().unwrap().restart_count,
            3,
            "restart_count must not exceed max_restarts"
        );
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn healthy_running_clears_stale_restart_count() {
        let state = make_state(RuntimeStatus::Running);
        state.write().unwrap().restart_count = 2;
        let config = RuntimeConfig {
            watchdog_ms: Some(1000),
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        // Keep cycle_count climbing for >60 simulated seconds so the watchdog
        // sees sustained progress and resets restart_count.
        for i in 1..=70u64 {
            bump_cycles(&state, i);
            pump(1, Duration::from_secs(1)).await;
        }
        assert_eq!(
            state.read().unwrap().restart_count,
            0,
            "restart_count should reset after 60 s of sustained healthy cycles"
        );
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn default_poll_interval_when_watchdog_ms_unset() {
        // No watchdog_ms → falls back to 1 s poll. Just smoke-test that the
        // loop starts and an Idle task survives a few virtual seconds.
        let state = make_state(RuntimeStatus::Idle);
        let config = RuntimeConfig {
            watchdog_ms: None,
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        pump(5, Duration::from_secs(1)).await;
        assert_eq!(state.read().unwrap().status, RuntimeStatus::Idle);
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn poll_interval_caps_at_1s() {
        // watchdog_ms > 1000 is clamped to 1 s — verify behavior via timing.
        let state = make_state(RuntimeStatus::Running);
        state.write().unwrap().restart_count = 1;
        let config = RuntimeConfig {
            watchdog_ms: Some(10_000), // caller asks for 10s, should cap at 1s
            ..RuntimeConfig::default()
        };
        let (wd, _rx) = make_watchdog(Arc::clone(&state), config);
        let handle = tokio::spawn(wd.run());
        // Bump cycles for 70 s. If the poll were 10 s, we'd have ~7 ticks;
        // with the 1 s cap, we have ~70 ticks and healthy_since elapses.
        for i in 1..=70u64 {
            bump_cycles(&state, i);
            pump(1, Duration::from_secs(1)).await;
        }
        assert_eq!(state.read().unwrap().restart_count, 0);
        handle.abort();
    }
}
