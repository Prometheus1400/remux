use std::{
    env::var,
    fs::{File, create_dir_all},
    path::PathBuf,
};

use fs2::FileExt;

use crate::{
    constants::{self, HOME_DIR, RUNTIME_DIR},
    error::{Error, Result},
};

const DAEMON_LOCK_FILE_ENV: &str = "REMUX_DAEMON_LOCK_FILE";

/// if we can't lock the daemon file then the daemon
/// process must be running
pub fn is_daemon_running() -> bool {
    lock_daemon_file().is_err()
}

pub fn get_daemon_file() -> Result<File> {
    let path = var(DAEMON_LOCK_FILE_ENV).unwrap_or_else(|_| constants::DAEMON_LOCK_FILE.to_owned());
    let file = File::create(path)?;
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
            create_dir_all(parent)?;
        }

        return Ok(path);
    }

    Err(Error::MissingSocketPathEnv)
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use std::{
        fs,
        sync::{LazyLock, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn temp_home_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "remux-{test_name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        ))
    }

    fn with_temp_lock_file<T>(test_name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let _lock = TEST_MUTEX.lock();
        let path = temp_home_path(test_name);

        unsafe {
            std::env::set_var(DAEMON_LOCK_FILE_ENV, &path);
        }

        let result = f();

        unsafe {
            std::env::remove_var(DAEMON_LOCK_FILE_ENV);
        }
        let _ = fs::remove_file(&path);

        result
    }

    #[test]
    fn test_get_daemon_file() {
        with_temp_lock_file("daemon-file", || {
            get_daemon_file().unwrap();
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_lock_daemon_file_success() -> Result<()> {
        with_temp_lock_file("lock-success", || {
            lock_daemon_file()?;
            Ok(())
        })
    }

    #[test]
    fn test_lock_daemon_file_failure() -> Result<()> {
        with_temp_lock_file("lock-failure", || {
            let _locked_file = lock_daemon_file()?;
            assert!(lock_daemon_file().is_err());
            Ok(())
        })
    }

    #[test]
    fn get_sock_path_prefers_runtime_dir_when_present() -> Result<()> {
        let _lock = TEST_MUTEX.lock();
        let runtime_dir = temp_home_path("runtime-dir");
        fs::create_dir_all(&runtime_dir)?;

        unsafe {
            std::env::set_var(RUNTIME_DIR, &runtime_dir);
            std::env::set_var(HOME_DIR, temp_home_path("home-ignored"));
        }

        let path = get_sock_path()?;

        assert_eq!(path, runtime_dir.join("remux.sock"));

        unsafe {
            std::env::remove_var(RUNTIME_DIR);
            std::env::remove_var(HOME_DIR);
        }
        fs::remove_dir_all(runtime_dir)?;
        Ok(())
    }

    #[test]
    fn get_sock_path_creates_home_based_run_directory() -> Result<()> {
        let _lock = TEST_MUTEX.lock();
        let home_dir = temp_home_path("home-dir");

        unsafe {
            std::env::remove_var(RUNTIME_DIR);
            std::env::set_var(HOME_DIR, &home_dir);
        }

        let path = get_sock_path()?;

        assert_eq!(path, home_dir.join(".remux/run/remux.sock"));
        assert!(path.parent().expect("socket path should have a parent").exists());

        unsafe {
            std::env::remove_var(HOME_DIR);
        }
        fs::remove_dir_all(home_dir)?;
        Ok(())
    }
}
