use std::process::Command;

use anyhow::{Context, Result};

/// Read text from the Wayland clipboard via `wl-paste`.
pub fn read_clipboard_text() -> Result<String> {
    let text = read_clipboard_command("wl-paste", &[])?;
    if !text.is_empty() {
        Ok(text)
    } else {
        anyhow::bail!("Clipboard is empty or unavailable")
    }
}

/// Read clipboard via an external command.
/// When running as root under sudo, always uses the original user's Wayland session.
fn read_clipboard_command(cmd: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(cmd);
    command.args(args);

    let output = command
        .output()
        .with_context(|| format!("Failed to execute {}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} failed: {} (stderr: {})",
            cmd,
            output.status,
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
