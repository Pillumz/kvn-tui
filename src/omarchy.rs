//! Filesystem-level detection of an Omarchy installation.
//!
//! Kept as a neutral top-level module so both the daemon-side config loader
//! (which sets the first-launch default theme) and the TUI-side theme watcher
//! (which follows Omarchy's active theme at runtime) can share it without
//! `config` having to depend on `tui_client`.

use std::path::PathBuf;

/// Read the currently active Omarchy theme slug.
///
/// Omarchy 4 stores runtime state below
/// `$XDG_STATE_HOME/omarchy/current/theme.name`; Omarchy 3 used
/// `$XDG_CONFIG_HOME/omarchy/current/theme.name`. The v4 path wins when both
/// exist so stale files retained after an upgrade cannot override the live
/// theme. Returns `None` when neither file contains a theme name.
pub fn detect_omarchy_theme() -> Option<String> {
    for current in current_dir_candidates() {
        let Ok(raw) = std::fs::read_to_string(current.join("theme.name")) else {
            continue;
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Path to the active Omarchy `current/` state directory.
///
/// The Omarchy 4 XDG state path is preferred, with the Omarchy 3 config path
/// retained as a compatibility fallback.
pub fn omarchy_current_dir() -> Option<PathBuf> {
    let candidates = current_dir_candidates();
    candidates
        .iter()
        .find(|path| path.join("theme.name").is_file())
        .or_else(|| candidates.iter().find(|path| path.is_dir()))
        .cloned()
        .or_else(|| candidates.into_iter().next())
}

/// Path to the active Omarchy `theme.name` file.
pub fn theme_name_path() -> Option<PathBuf> {
    Some(omarchy_current_dir()?.join("theme.name"))
}

fn current_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(state) = state_home() {
        candidates.push(state.join("omarchy").join("current"));
    }
    if let Some(config) = config_home() {
        candidates.push(config.join("omarchy").join("current"));
    }
    candidates
}

fn state_home() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(xdg);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    dirs::state_dir()
}

fn config_home() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    dirs::config_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::ENV_LOCK;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn isolate_xdg(config: &std::path::Path, state: &std::path::Path) -> (EnvGuard, EnvGuard) {
        (
            EnvGuard::set("XDG_CONFIG_HOME", config),
            EnvGuard::set("XDG_STATE_HOME", state),
        )
    }

    #[test]
    fn detect_returns_none_when_file_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let _env = isolate_xdg(config.path(), state.path());
        assert!(detect_omarchy_theme().is_none());
    }

    #[test]
    fn detect_reads_legacy_config_theme_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let _env = isolate_xdg(config.path(), state.path());
        let current = config.path().join("omarchy").join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("theme.name"), "  gruvbox  \n").unwrap();
        assert_eq!(detect_omarchy_theme().as_deref(), Some("gruvbox"));
        assert_eq!(omarchy_current_dir().as_deref(), Some(current.as_path()));
    }

    #[test]
    fn detect_prefers_omarchy_four_state_theme_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let _env = isolate_xdg(config.path(), state.path());
        let legacy = config.path().join("omarchy").join("current");
        let current = state.path().join("omarchy").join("current");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(legacy.join("theme.name"), "gruvbox\n").unwrap();
        std::fs::write(current.join("theme.name"), "lupine\n").unwrap();
        assert_eq!(detect_omarchy_theme().as_deref(), Some("lupine"));
        assert_eq!(theme_name_path(), Some(current.join("theme.name")));
    }

    #[test]
    fn detect_treats_empty_file_as_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let _env = isolate_xdg(config.path(), state.path());
        let current = state.path().join("omarchy").join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("theme.name"), "   \n").unwrap();
        assert!(detect_omarchy_theme().is_none());
    }
}
