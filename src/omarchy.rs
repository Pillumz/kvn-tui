//! Filesystem-level detection of an Omarchy installation.
//!
//! Kept as a neutral top-level module so both the daemon-side config loader
//! (which sets the first-launch default theme) and the TUI-side theme watcher
//! (which follows Omarchy's active theme at runtime) can share it without
//! `config` having to depend on `tui_client`.

use std::path::PathBuf;

/// Read the currently active Omarchy theme slug from
/// `$XDG_CONFIG_HOME/omarchy/current/theme.name`, with the conventional
/// fallback to `~/.config`. Returns `None` on non-Omarchy systems (file
/// missing) or when the file is empty.
pub fn detect_omarchy_theme() -> Option<String> {
    let path = theme_name_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Path to `$XDG_CONFIG_HOME/omarchy/current/`. Whole directory is swapped
/// atomically on theme change, so the theme watcher watches this parent
/// rather than the file inside it.
pub fn omarchy_current_dir() -> Option<PathBuf> {
    Some(config_home()?.join("omarchy").join("current"))
}

/// Path to `$XDG_CONFIG_HOME/omarchy/current/theme.name`.
pub fn theme_name_path() -> Option<PathBuf> {
    Some(omarchy_current_dir()?.join("theme.name"))
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

    #[test]
    fn detect_returns_none_when_file_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        assert!(detect_omarchy_theme().is_none());
    }

    #[test]
    fn detect_reads_trimmed_slug_from_theme_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let current = dir.path().join("omarchy").join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("theme.name"), "  gruvbox  \n").unwrap();
        assert_eq!(detect_omarchy_theme().as_deref(), Some("gruvbox"));
    }

    #[test]
    fn detect_treats_empty_file_as_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let current = dir.path().join("omarchy").join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("theme.name"), "   \n").unwrap();
        assert!(detect_omarchy_theme().is_none());
    }
}
