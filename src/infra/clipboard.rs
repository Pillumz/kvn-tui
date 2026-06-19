use std::io::Write;
use std::process::{Command, Stdio};

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

/// Write text to the Wayland clipboard via `wl-copy`.
///
/// `wl-copy` reads stdin, then forks itself into a background daemon that
/// serves the selection until something else claims the clipboard. We must
/// (a) close our stdin handle before waiting so wl-copy can finish reading,
/// and (b) redirect stdout/stderr to /dev/null so the inherited fds in the
/// background daemon don't keep their pipes open forever (which would hang
/// `wait_with_output`). We use `wait()` here (parent exits promptly after
/// forking the daemon) rather than `wait_with_output`.
pub fn write_clipboard_text(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn wl-copy")?;
    {
        let mut stdin = child.stdin.take().context("wl-copy stdin not available")?;
        stdin
            .write_all(text.as_bytes())
            .context("Failed to write to wl-copy stdin")?;
        // stdin dropped here, closing the pipe so wl-copy proceeds.
    }
    let status = child.wait().context("Failed to wait for wl-copy")?;
    if !status.success() {
        anyhow::bail!("wl-copy failed: {}", status);
    }
    Ok(())
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
