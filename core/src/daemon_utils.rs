use std::fs::File;

use crate::constants;
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
