use std::env::var;
use std::fs::{File, create_dir_all};
use std::path::PathBuf;

use crate::constants::{self, HOME_DIR, RUNTIME_DIR};
use crate::error::{Error, Result};
use fs2::FileExt;

/// if we can't lock the daemon file then the daemon
/// process must be running
pub fn is_daemon_running() -> bool {
    lock_daemon_file().is_err()
}

pub fn get_daemon_file() -> Result<File> {
    let file = File::create(constants::DAEMON_LOCK_FILE)?;
    Ok(file)
}

pub fn lock_daemon_file() -> Result<File> {
    let file = get_daemon_file()?;
    file.try_lock_exclusive()?;
    Ok(file)
}

pub fn get_sock_path() -> Result<PathBuf> {
    // For linux systems
    if let Ok(runtime_dir) = var(RUNTIME_DIR) {
        return Ok(PathBuf::from(runtime_dir).join("remux.sock"));
    }

    if let Ok(home_dir) = var(HOME_DIR) {
        let path = PathBuf::from(home_dir).join(".remux/run/remux.sock");

        if let Some(parent) = path.parent() {
            create_dir_all(parent).map_err(|e| Error::DaemonFileError(e))?;
        }

        return Ok(path);
    }

    Err(Error::Custom(
        "Could not determine socket path: neither XDG_RUNTIME_DIR nor HOME are set".to_string(),
    ))
}

#[cfg(test)]
mod test {
    type Error = Box<dyn std::error::Error>;
    type Result<T> = std::result::Result<T, Error>;
    use std::sync::{LazyLock, Mutex};

    use super::*;

    static TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn test_get_daemon_file() {
        get_daemon_file().unwrap();
    }

    // following tests operate on the shared lock file resource
    #[test]
    fn test_lock_daemon_file_success() -> Result<()> {
        let _lock = TEST_MUTEX.lock();
        lock_daemon_file()?;
        Ok(())
    }

    #[test]
    fn test_lock_daemon_file_failure() -> Result<()> {
        let _lock = TEST_MUTEX.lock();
        let _locked_file = lock_daemon_file()?;
        assert!(lock_daemon_file().is_err());
        Ok(())
    }
}
