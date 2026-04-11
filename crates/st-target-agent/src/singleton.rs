//! Process singleton enforcement via PID file + advisory lock.
//!
//! Prevents two `st-runtime agent` instances from running simultaneously.
//! Two instances controlling the same physical I/O can cause machinery
//! damage or personal injury.
//!
//! Uses `flock(LOCK_EX | LOCK_NB)` on a PID file. The advisory lock is
//! automatically released by the kernel if the process crashes — no stale
//! PID file cleanup needed.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// Error returned when singleton acquisition fails.
#[derive(Debug)]
pub enum SingletonError {
    /// Another instance is already running.
    AlreadyRunning { pid: String },
    /// I/O error during lock acquisition.
    IoError(std::io::Error),
}

impl std::fmt::Display for SingletonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingletonError::AlreadyRunning { pid } => {
                write!(f, "another instance is already running (PID {pid})")
            }
            SingletonError::IoError(e) => write!(f, "PID lock I/O error: {e}"),
        }
    }
}

/// RAII guard that holds the singleton lock. The lock is released when
/// this guard is dropped (either on clean shutdown or panic unwind).
#[derive(Debug)]
pub struct SingletonGuard {
    _file: File, // Keeps the flock alive
    path: PathBuf,
}

impl SingletonGuard {
    /// Attempt to acquire the singleton lock.
    ///
    /// Creates the PID file at `pid_path`, acquires an exclusive non-blocking
    /// flock, and writes the current PID. Returns `Err(AlreadyRunning)` if
    /// another process holds the lock.
    pub fn acquire(pid_path: &Path) -> Result<Self, SingletonError> {
        // Ensure parent directory exists
        if let Some(parent) = pid_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(pid_path)
            .map_err(SingletonError::IoError)?;

        // Try non-blocking exclusive lock
        let fd = file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                // Another process holds the lock — read its PID
                let pid = fs::read_to_string(pid_path)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                return Err(SingletonError::AlreadyRunning {
                    pid: if pid.is_empty() {
                        "unknown".to_string()
                    } else {
                        pid
                    },
                });
            }
            return Err(SingletonError::IoError(err));
        }

        // Lock acquired — write our PID
        // Truncate first (the file may contain a stale PID from a previous run
        // that crashed after writing but before the kernel released the lock
        // on the next startup).
        let _ = file.set_len(0);
        let mut f = &file;
        let _ = write!(f, "{}", std::process::id());

        Ok(SingletonGuard {
            _file: file,
            path: pid_path.to_path_buf(),
        })
    }
}

impl Drop for SingletonGuard {
    fn drop(&mut self) {
        // Clean up PID file on graceful shutdown.
        // On crash, the kernel releases the flock automatically.
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_acquire_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        let guard = SingletonGuard::acquire(&pid_path).expect("Should acquire");

        // PID file should contain our PID
        let mut content = String::new();
        File::open(&pid_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content.trim(), std::process::id().to_string());

        drop(guard);
    }

    #[test]
    fn test_double_acquire_fails() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        let _guard1 = SingletonGuard::acquire(&pid_path).expect("First acquire should succeed");

        match SingletonGuard::acquire(&pid_path) {
            Err(SingletonError::AlreadyRunning { pid }) => {
                assert_eq!(pid, std::process::id().to_string());
            }
            other => panic!("Expected AlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn test_drop_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        {
            let _guard = SingletonGuard::acquire(&pid_path).expect("First acquire");
        } // guard dropped here

        // Should be able to acquire again
        let _guard2 = SingletonGuard::acquire(&pid_path).expect("Second acquire after drop");
    }

    #[test]
    fn test_stale_file_without_lock() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        // Write a stale PID file (no lock held)
        fs::write(&pid_path, "99999").unwrap();

        // Should succeed because the file has no flock
        let guard = SingletonGuard::acquire(&pid_path).expect("Should acquire despite stale file");

        // Verify our PID overwrote the stale one
        let mut content = String::new();
        File::open(&pid_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content.trim(), std::process::id().to_string());

        drop(guard);
    }
}
