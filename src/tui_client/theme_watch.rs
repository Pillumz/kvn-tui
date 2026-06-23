//! Detect and follow the active Omarchy theme.
//!
//! Omarchy stores its active theme as a single-line slug in
//! `~/.config/omarchy/current/theme.name`. On theme change the whole
//! `current/` directory is atomically replaced, so we watch the **parent**
//! directory (`omarchy/`) rather than the file itself — watching a file that
//! gets unlinked-then-recreated is unreliable across most file-watcher
//! backends.
//!
//! On non-Omarchy systems neither the directory nor the file exists, so the
//! detector returns `None` and the watcher exits immediately without
//! spawning any background work.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::app::msg::Msg;
use crate::ui::styles::Theme;

/// Sentinel slug stored in `Settings.theme` to mean "follow Omarchy's
/// current theme.name". The in-TUI picker writes this when the user
/// selects the "Auto (Omarchy)" entry.
pub const OMARCHY_SENTINEL: &str = "omarchy";

/// Default fallback theme used when `OMARCHY_SENTINEL` is set but
/// Omarchy isn't installed. Matches `Settings::default().theme`.
pub const DEFAULT_THEME: &str = "tokyo-night";

/// Read the currently active Omarchy theme slug from
/// `$XDG_CONFIG_HOME/omarchy/current/theme.name`, with the conventional
/// fallback to `~/.config`.
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

/// Resolve a `Settings.theme` slug into a concrete [`Theme`]. The
/// reserved slug [`OMARCHY_SENTINEL`] means "follow Omarchy's current
/// theme.name"; on systems without Omarchy it falls back to
/// [`DEFAULT_THEME`]. Any other slug is looked up as a bundled palette
/// (with `Theme::resolve` defaulting to legacy for unknown names).
pub fn resolve_active(settings_theme: &str) -> Theme {
    if settings_theme == OMARCHY_SENTINEL {
        match detect_omarchy_theme() {
            Some(name) => Theme::resolve(&name),
            None => Theme::resolve(DEFAULT_THEME),
        }
    } else {
        Theme::resolve(settings_theme)
    }
}

/// Spawn a background thread that watches Omarchy's `current/` directory
/// for theme changes and sends [`Msg::ThemeChanged`] when the active slug
/// changes. Returns immediately and does nothing if Omarchy isn't installed.
pub fn spawn_theme_watcher(tx: Sender<Msg>) {
    let Some(theme_file) = theme_name_path() else {
        return;
    };
    let Some(watch_dir) = omarchy_current_dir() else {
        return;
    };
    if !watch_dir.exists() {
        return;
    }

    thread::spawn(move || run_watcher(tx, watch_dir, theme_file));
}

fn run_watcher(tx: Sender<Msg>, watch_dir: PathBuf, theme_file: PathBuf) {
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = notify_tx.send(res);
    }) {
        Ok(w) => w,
        Err(_) => return,
    };
    if watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .is_err()
    {
        return;
    }

    let mut last_slug = std::fs::read_to_string(&theme_file)
        .ok()
        .map(|s| s.trim().to_string());

    loop {
        // Block until at least one event arrives, then drain any follow-ups
        // (Omarchy's atomic swap fires several events in quick succession).
        let Ok(_first) = notify_rx.recv() else {
            return;
        };
        while notify_rx.recv_timeout(Duration::from_millis(50)).is_ok() {}

        let current = std::fs::read_to_string(&theme_file)
            .ok()
            .map(|s| s.trim().to_string());
        if current != last_slug {
            if let Some(ref slug) = current {
                if tx.send(Msg::ThemeChanged(Theme::resolve(slug))).is_err() {
                    return;
                }
            }
            last_slug = current;
        }
    }
}

fn omarchy_current_dir() -> Option<PathBuf> {
    Some(config_home()?.join("omarchy").join("current"))
}

fn theme_name_path() -> Option<PathBuf> {
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

    /// `ENV_LOCK` is shared with other tests that mutate `XDG_CONFIG_HOME`.
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

    #[test]
    fn resolve_active_uses_named_slug_directly() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let theme = resolve_active("gruvbox");
        // Gruvbox accent is #7daea3 — matches themes/gruvbox.toml.
        assert_eq!(
            theme.accent().fg,
            Some(ratatui::style::Color::Rgb(0x7d, 0xae, 0xa3))
        );
    }

    #[test]
    fn resolve_active_omarchy_sentinel_without_file_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let theme = resolve_active(OMARCHY_SENTINEL);
        // Default fallback is tokyo-night (#7aa2f7 accent).
        assert_eq!(
            theme.accent().fg,
            Some(ratatui::style::Color::Rgb(0x7a, 0xa2, 0xf7))
        );
    }

    #[test]
    fn resolve_active_omarchy_sentinel_reads_theme_name_when_present() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let current = dir.path().join("omarchy").join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("theme.name"), "nord\n").unwrap();
        let theme = resolve_active(OMARCHY_SENTINEL);
        // Nord accent #81a1c1.
        assert_eq!(
            theme.accent().fg,
            Some(ratatui::style::Color::Rgb(0x81, 0xa1, 0xc1))
        );
    }
}
