use std::env::var;
use std::fs::{File, create_dir_all};
use std::path::PathBuf;

use crate::constants::{self, HOME_DIR, RUNTIME_DIR};
use crate::error::RemuxLibError;
use fs2::FileExt;

/// if we can't lock the daemon file then the daemon
/// process must be running
pub fn is_daemon_running() -> bool {
    lock_daemon_file().is_err()
}

pub fn get_daemon_file() -> Result<File, RemuxLibError> {
    let file = File::create(constants::DAEMON_LOCK_FILE)?;
    Ok(file)
}

pub fn lock_daemon_file() -> Result<File, RemuxLibError> {
    let file = get_daemon_file()?;
    file.try_lock_exclusive()?;
    Ok(file)
}

pub fn get_sock_path() -> Result<PathBuf, RemuxLibError> {
    // For linux systems
    if let Ok(runtime_dir) = var(RUNTIME_DIR) {
        return Ok(PathBuf::from(runtime_dir).join("remux.sock"));
    }

    if let Ok(home_dir) = var(HOME_DIR) {
        let path = PathBuf::from(home_dir).join(".remux/run/remux.sock");

        if let Some(parent) = path.parent() {
            create_dir_all(parent).map_err(|e| RemuxLibError::DaemonFileError(e))?;
        }

        return Ok(path);
    }

    Err(RemuxLibError::UnixSocketError(
        "Could not determine socket path: neither XDG_RUNTIME_DIR nor HOME are set".to_string(),
    ))
}

#[cfg(test)]
mod test {
    use std::sync::{LazyLock, Mutex};

    use super::*;

    static TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn test_get_daemon_file() {
        get_daemon_file().unwrap();
    }

    // following tests operate on the shared lock file resource
    #[test]
    fn test_lock_daemon_file_success() {
        let _lock = TEST_MUTEX.lock();
        lock_daemon_file().unwrap();
    }

    #[test]
    fn test_lock_daemon_file_failure() {
        let _lock = TEST_MUTEX.lock();
        let _locked_file = lock_daemon_file().unwrap();
        assert!(lock_daemon_file().is_err());
    }
}
