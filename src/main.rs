#![warn(unsafe_code)]
// Tests touch `std::env::set_var` (Rust 2024 unsafe) and other test-only
// shims that are inherently `unsafe`. These are scoped through
// `test_helpers::ENV_LOCK` and don't need to fight the lint.
#![cfg_attr(test, allow(unsafe_code))]

mod app;
mod cli;
mod config;
mod daemon;
mod infra;
mod ipc;
mod services;
mod singbox;
mod tui_client;
mod ui;

#[cfg(test)]
mod test_helpers;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::app::model::Model;
use crate::infra::paths::ensure_config_dirs;

/// Entry point for the TUI VPN client.
fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    if let Some(result) = cli::try_run_from_parsed(&cli) {
        return result;
    }

    // Initialize logging.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    // Ensure configuration directories exist.
    ensure_config_dirs()?;

    if cli.daemon {
        let model = Model::new()?;
        daemon::run(model)?;
    } else {
        if !ipc::is_daemon_running() {
            spawn_daemon_process()?;
            if !ipc::wait_for_daemon(std::time::Duration::from_millis(2000)) {
                anyhow::bail!("daemon failed to start within 2s");
            }
        }
        tui_client::run()?;
    }

    Ok(())
}

/// Re-exec ourselves as `kvn-tui --daemon` in a fresh process group so the
/// daemon outlives the TUI. The previous implementation spawned the daemon
/// as a thread inside the TUI process — once the user pressed `q`, the
/// process exited and took the daemon (and sing-box) with it. In the normal
/// flow this is dead code (the hyprland autostart already runs `--daemon`),
/// but it matters when that autostart hasn't fired yet.
fn spawn_daemon_process() -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().context("Failed to determine current executable path")?;
    Command::new(exe)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("Failed to spawn daemon process")?;
    Ok(())
}
