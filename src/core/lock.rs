use crate::error::{AppError, Result};
use fs4::{FileExt, TryLockError};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Advisory exclusive lock for single-writer index coordination on `<out_dir>/.lock`.
pub struct IndexLock;

/// RAII guard representing exclusive ownership of the index lock.
/// Unlocks automatically when dropped. The lock file itself is never deleted.
pub struct IndexLockGuard {
    file: File,
    pub path: PathBuf,
}

impl Drop for IndexLockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl IndexLock {
    /// Acquire an exclusive advisory lock on `<out_dir>/.lock`.
    /// Waits up to `timeout` before returning an error.
    /// Never deletes the lock file.
    pub fn acquire(out_dir: &Path, timeout: Duration) -> Result<IndexLockGuard> {
        if !out_dir.exists() {
            fs::create_dir_all(out_dir)?;
        }
        let lock_path = out_dir.join(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        let start = Instant::now();
        loop {
            match FileExt::try_lock(&file) {
                Ok(()) => {
                    return Ok(IndexLockGuard {
                        file,
                        path: lock_path,
                    });
                }
                Err(TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        return Err(AppError::Index(format!(
                            "Timed out after {:?} waiting for lock on {:?}",
                            timeout, lock_path
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::Error(err)) => {
                    return Err(AppError::Io(err));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_lock_acquire_and_release_on_drop() {
        let temp = tempdir().expect("tempdir");
        let out_dir = temp.path();

        // First acquire should succeed
        let guard1 = IndexLock::acquire(out_dir, Duration::from_millis(100))
            .expect("First acquire must succeed");

        assert!(guard1.path.exists(), "Lock file must exist on disk");

        // Second acquire while guard1 is held must time out
        let start = Instant::now();
        let guard2_res = IndexLock::acquire(out_dir, Duration::from_millis(80));
        assert!(
            guard2_res.is_err(),
            "Second acquire must time out while first is held"
        );
        assert!(
            start.elapsed() >= Duration::from_millis(70),
            "Must wait for timeout"
        );

        // Drop guard1
        drop(guard1);

        // Lock file must not be deleted on drop
        assert!(
            out_dir.join(".lock").exists(),
            "Lock file must never be deleted"
        );

        // Now acquire should succeed immediately
        let guard3 = IndexLock::acquire(out_dir, Duration::from_millis(100))
            .expect("Acquire after drop must succeed");
        drop(guard3);
    }

    #[test]
    fn test_cross_process_exclusion() {
        let temp = tempdir().expect("tempdir");
        let out_dir = temp.path();

        // Spawn python child process holding an exclusive lock on the lock file
        let lock_file = out_dir.join(".lock");
        // Ensure directory and lock file exist
        fs::create_dir_all(out_dir).unwrap();
        File::create(&lock_file).unwrap();

        let lock_path_str = lock_file.to_str().unwrap().to_string();
        let py_script = format!(
            "import fcntl, time, sys\nf = open('{}', 'r+')\nfcntl.flock(f, fcntl.LOCK_EX)\nsys.stdout.write('LOCKED\\n')\nsys.stdout.flush()\ntime.sleep(0.5)\n",
            lock_path_str
        );

        let mut child = std::process::Command::new("python3")
            .arg("-c")
            .arg(&py_script)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn python lock holder");

        // Wait until child signals LOCKED
        use std::io::{BufRead, BufReader};
        let stdout = child.stdout.take().expect("child stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read line");
        assert_eq!(line.trim(), "LOCKED");

        // Try to acquire while child holds the lock -> must time out
        let res = IndexLock::acquire(out_dir, Duration::from_millis(50));
        assert!(res.is_err(), "Must time out while child process holds lock");

        // Wait for child to exit and release lock
        let status = child.wait().expect("child wait");
        assert!(status.success());

        // Now parent should be able to acquire lock
        let guard = IndexLock::acquire(out_dir, Duration::from_millis(200));
        assert!(
            guard.is_ok(),
            "Must acquire lock after child process releases it"
        );
    }
}
