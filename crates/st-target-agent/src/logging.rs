//! Logging initialization with journald and runtime log level control.
//!
//! On Linux targets, logs are sent to systemd's journald via the native
//! journal socket. This means:
//! - No log files to manage (journald handles rotation and compression)
//! - Logs are queryable with `journalctl -u st-runtime`
//! - Structured fields (unit name, priority) are preserved
//! - Log level can be changed at runtime via the HTTP API
//!
//! If journald is not available (e.g., in tests or non-systemd systems),
//! falls back to stderr logging.

use std::str::FromStr;
use std::sync::Arc;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Handle for changing the log level at runtime.
#[derive(Clone)]
pub struct LogLevelHandle {
    reload_handle: Arc<reload::Handle<EnvFilter, tracing_subscriber::Registry>>,
    current_level: Arc<std::sync::RwLock<String>>,
}

impl LogLevelHandle {
    /// Get the current log level as a string.
    pub fn current_level(&self) -> String {
        self.current_level.read().unwrap().clone()
    }

    /// Change the log level at runtime. Returns Ok on success, Err with message on invalid level.
    pub fn set_level(&self, level: &str) -> Result<(), String> {
        // Validate the level string
        let _ = LevelFilter::from_str(level)
            .map_err(|_| format!("Invalid log level: '{level}'. Use: trace, debug, info, warn, error"))?;

        // Try to reload the actual subscriber filter. This may fail if the
        // subscriber was already set by another init_logging() call (e.g., in
        // tests). We still track the level internally regardless.
        let filter = EnvFilter::new(level);
        let _ = self.reload_handle.reload(filter);

        *self.current_level.write().unwrap() = level.to_string();
        tracing::info!("Log level changed to: {level}");
        Ok(())
    }
}

/// Initialize the logging subsystem.
///
/// Tries journald first (native on systemd Linux). Falls back to stderr
/// if journald is not available (tests, non-systemd systems).
///
/// Returns a `LogLevelHandle` for runtime level changes via the API.
pub fn init_logging(initial_level: &str) -> LogLevelHandle {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(initial_level));

    let (filter_layer, reload_handle) = reload::Layer::new(filter);

    // Try journald first
    let journald_result = tracing_journald::layer();
    let init_result = match journald_result {
        Ok(journald_layer) => {
            tracing_subscriber::registry()
                .with(filter_layer)
                .with(journald_layer)
                .try_init()
        }
        Err(_) => {
            // Fallback to stderr (tests, non-systemd)
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false);

            tracing_subscriber::registry()
                .with(filter_layer)
                .with(fmt_layer)
                .try_init()
        }
    };

    match init_result {
        Ok(()) => eprintln!("[logging] Initialized (level: {initial_level})"),
        Err(_) => eprintln!("[logging] Global subscriber already set, using reload handle only"),
    }

    LogLevelHandle {
        reload_handle: Arc::new(reload_handle),
        current_level: Arc::new(std::sync::RwLock::new(initial_level.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_log_levels() {
        // These should all be valid
        for level in &["trace", "debug", "info", "warn", "error"] {
            assert!(
                LevelFilter::from_str(level).is_ok(),
                "{level} should be valid"
            );
        }
    }

    #[test]
    fn invalid_level_rejected() {
        assert!(LevelFilter::from_str("verbose").is_err());
        assert!(LevelFilter::from_str("foobar").is_err());
    }
}
