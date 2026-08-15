//! Follow the active Omarchy theme at runtime.
//!
//! Omarchy 4 stores its active theme as a single-line slug in
//! `~/.local/state/omarchy/current/theme.name`; Omarchy 3 used
//! `~/.config/omarchy/current/theme.name`. The shared detection helper picks
//! the active layout and this watcher follows its `current/` directory.
//!
//! On non-Omarchy systems the watched directory does not exist and the
//! watcher exits immediately without spawning any background work.
//!
//! The filesystem-level detection helpers live in [`crate::omarchy`] so the
//! daemon-side config loader can share them without depending on the TUI.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::app::msg::Msg;
use crate::omarchy::{detect_omarchy_theme, omarchy_current_dir, theme_name_path};
use crate::ui::styles::Theme;

/// Sentinel slug stored in `Settings.theme` to mean "follow Omarchy's
/// current theme.name". The in-TUI picker writes this when the user
/// selects the "Auto (Omarchy)" entry.
pub const OMARCHY_SENTINEL: &str = "omarchy";

/// Default fallback theme used when `OMARCHY_SENTINEL` is set but
/// Omarchy isn't installed. Matches `Settings::default().theme`.
pub const DEFAULT_THEME: &str = "tokyo-night";

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

/// Spawn a background thread that watches Omarchy's active `current/` directory
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `ENV_LOCK` is shared with other tests that mutate `XDG_CONFIG_HOME`.
    use crate::test_helpers::ENV_LOCK;

    #[test]
    fn resolve_active_uses_named_slug_directly() {
        let _guard = ENV_LOCK.lock().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
        unsafe { std::env::set_var("XDG_STATE_HOME", state.path()) };
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
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
        unsafe { std::env::set_var("XDG_STATE_HOME", state.path()) };
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
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
        unsafe { std::env::set_var("XDG_STATE_HOME", state.path()) };
        let current = state.path().join("omarchy").join("current");
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
