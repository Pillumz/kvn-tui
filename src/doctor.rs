//! Read-only diagnostics for the runtime environment.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;

const MIN_SINGBOX_VERSION: (u64, u64, u64) = (1, 12, 0);
const USER_UNIT: &str = "kvn-tui.service";
const KILLSWITCH_HELPER: &str = "/usr/lib/kvn-tui/killswitch-helper.sh";
const POLKIT_DNS_ACTION: &str = "org.freedesktop.resolve1.set-dns-servers";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Pass,
    Warning,
    Failure,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    level: Level,
    message: String,
    remedy: Option<String>,
}

impl Check {
    fn pass(message: impl Into<String>) -> Self {
        Self::new(Level::Pass, message, None)
    }

    fn warning(message: impl Into<String>, remedy: impl Into<String>) -> Self {
        Self::new(Level::Warning, message, Some(remedy.into()))
    }

    fn failure(message: impl Into<String>, remedy: impl Into<String>) -> Self {
        Self::new(Level::Failure, message, Some(remedy.into()))
    }

    fn optional(message: impl Into<String>) -> Self {
        Self::new(Level::Optional, message, None)
    }

    fn new(level: Level, message: impl Into<String>, remedy: Option<String>) -> Self {
        Self {
            level,
            message: message.into(),
            remedy,
        }
    }
}

/// Run all diagnostics, print the report, and fail when a required component
/// is not usable.
pub fn run() -> Result<()> {
    let checks = collect();
    print_report(&checks);
    let failures = checks
        .iter()
        .filter(|check| check.level == Level::Failure)
        .count();
    if failures > 0 {
        anyhow::bail!("doctor found {failures} required check(s) that need attention");
    }
    Ok(())
}

fn collect() -> Vec<Check> {
    let mut checks = vec![Check::pass(format!(
        "kvn-tui {}",
        env!("CARGO_PKG_VERSION")
    ))];

    match find_singbox() {
        Some(path) => {
            checks.push(check_singbox_version(&path));
            checks.push(check_capabilities(&path));
        }
        None => checks.push(Check::failure(
            "sing-box was not found",
            "Install it with `sudo pacman -S sing-box` or set SING_BOX_PATH.",
        )),
    }

    checks.push(check_config());
    checks.extend(check_daemon());
    checks.push(check_clipboard());
    checks.push(check_killswitch());
    checks.push(check_polkit());
    checks.push(check_omarchy());
    checks
}

fn print_report(checks: &[Check]) {
    println!("kvn-tui doctor\n");
    for check in checks {
        let symbol = match check.level {
            Level::Pass => "✓",
            Level::Warning => "!",
            Level::Failure => "✗",
            Level::Optional => "○",
        };
        println!("{symbol} {}", check.message);
        if let Some(remedy) = &check.remedy {
            println!("  Fix: {remedy}");
        }
    }
}

fn find_singbox() -> Option<PathBuf> {
    if let Some(value) = env::var_os("SING_BOX_PATH") {
        let path = PathBuf::from(value);
        return executable_file(&path).then_some(path);
    }
    find_on_path("sing-box")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| executable_file(candidate))
}

fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn check_singbox_version(path: &Path) -> Check {
    let display = path.display();
    let output = Command::new(path).arg("version").output();
    let Ok(output) = output else {
        return Check::failure(
            format!("sing-box found at {display}, but could not be executed"),
            "Check the binary permissions or SING_BOX_PATH.",
        );
    };
    if !output.status.success() {
        return Check::failure(
            format!("sing-box found at {display}, but `version` failed"),
            "Reinstall sing-box and run `kvn-tui doctor` again.",
        );
    }

    let text = combined_output(&output);
    match parse_version(&text) {
        Some(version) if version >= MIN_SINGBOX_VERSION => Check::pass(format!(
            "sing-box found: {display} ({})",
            format_version(version)
        )),
        Some(version) => Check::failure(
            format!(
                "sing-box {} is too old; version 1.12.0 or newer is required",
                format_version(version)
            ),
            "Upgrade it with `sudo pacman -Syu sing-box`.",
        ),
        None => Check::failure(
            format!("could not determine the sing-box version from {display}"),
            "Run `sing-box version` and verify that the installation is valid.",
        ),
    }
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let candidate = word
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
            .trim_start_matches('v');
        let core = candidate
            .split_once('-')
            .map_or(candidate, |(core, _)| core);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        Some((major, minor, patch))
    })
}

fn format_version(version: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

fn check_capabilities(path: &Path) -> Check {
    let output = Command::new("getcap").arg(path).output();
    let Ok(output) = output else {
        return Check::warning(
            "could not inspect sing-box capabilities because `getcap` is unavailable",
            "Install `libcap` and run `kvn-tui doctor` again.",
        );
    };
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    if output.status.success()
        && text.contains("cap_net_admin")
        && text.contains("cap_net_raw")
        && (text.contains("=ep") || text.contains("+ep"))
    {
        Check::pass("sing-box has cap_net_admin and cap_net_raw")
    } else {
        Check::failure(
            "sing-box is missing the capabilities required for TUN mode",
            format!(
                "Run `sudo setcap cap_net_admin,cap_net_raw+ep {}`.",
                path.display()
            ),
        )
    }
}

fn check_config() -> Check {
    let Some(path) = crate::paths::profiles_path() else {
        return Check::failure(
            "the configuration directory could not be resolved",
            "Ensure HOME or XDG_CONFIG_HOME points to a writable directory.",
        );
    };
    if !path.exists() {
        return Check::pass(format!(
            "configuration will be created on first launch: {}",
            path.display()
        ));
    }
    match crate::config::load_config_at(&path).and_then(|config| config.validate()) {
        Ok(()) => Check::pass(format!("configuration is valid: {}", path.display())),
        Err(error) => Check::failure(
            format!("configuration is invalid: {error:#}"),
            format!("Correct {} or restore its backup.", path.display()),
        ),
    }
}

fn check_daemon() -> Vec<Check> {
    let mut checks = Vec::with_capacity(2);
    match systemctl_user_is_enabled() {
        Some(true) => checks.push(Check::pass("daemon autostart is enabled")),
        Some(false) => checks.push(Check::warning(
            "daemon autostart is not enabled",
            "Run `systemctl --user enable --now kvn-tui.service`.",
        )),
        None => checks.push(Check::warning(
            "systemd user service status could not be checked",
            "Run `systemctl --user status kvn-tui.service` to inspect it.",
        )),
    }

    if crate::ipc::is_daemon_running() {
        checks.push(Check::pass(format!(
            "daemon IPC socket is reachable: {}",
            crate::ipc::socket_path().display()
        )));
    } else {
        checks.push(Check::warning(
            "daemon IPC socket is not reachable",
            "Start it with `systemctl --user start kvn-tui.service`; kvn-tui can also start it on demand.",
        ));
    }
    checks
}

fn systemctl_user_is_enabled() -> Option<bool> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(["is-enabled", USER_UNIT])
        .output()
        .ok()?;
    if output.status.success() {
        return Some(true);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    matches!(stdout.trim(), "disabled" | "static" | "indirect").then_some(false)
}

fn check_clipboard() -> Check {
    let wayland = env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("x11"));
    if wayland && find_on_path("wl-paste").is_some() && find_on_path("wl-copy").is_some() {
        Check::pass("clipboard backend: wl-clipboard")
    } else if x11 && find_on_path("xclip").is_some() {
        Check::pass("clipboard backend: xclip")
    } else if x11 && find_on_path("xsel").is_some() {
        Check::pass("clipboard backend: xsel")
    } else if find_on_path("wl-paste").is_some() && find_on_path("wl-copy").is_some() {
        Check::pass("clipboard backend: wl-clipboard")
    } else if find_on_path("xclip").is_some() {
        Check::pass("clipboard backend: xclip")
    } else if find_on_path("xsel").is_some() {
        Check::pass("clipboard backend: xsel")
    } else {
        Check::warning(
            "no clipboard backend was found; import and export will be unavailable",
            "Install `wl-clipboard` on Wayland or `xclip`/`xsel` on X11.",
        )
    }
}

fn check_killswitch() -> Check {
    if Path::new(KILLSWITCH_HELPER).is_file() {
        Check::pass("kill switch helper is installed")
    } else {
        Check::optional("kill switch is not installed (optional)")
    }
}

fn check_polkit() -> Check {
    let Some(identity) = polkit_process_identity() else {
        return Check::warning(
            "polkit authorization could not be checked",
            "Run `kvn-tui doctor` again or inspect polkit with `sudo kvn-tui --install-polkit`.",
        );
    };
    let output = Command::new("pkcheck")
        .args(["--action-id", POLKIT_DNS_ACTION, "--process", &identity])
        .output();
    let Ok(output) = output else {
        return Check::warning(
            "`pkcheck` is unavailable, so polkit authorization could not be checked",
            "Install the `polkit` package and run `kvn-tui doctor` again.",
        );
    };

    match output.status.code() {
        Some(0) => Check::pass("polkit authorization for DNS changes is active"),
        // 1 means denied. 2 means authorization would require interaction;
        // doctor deliberately never opens an authentication prompt.
        Some(1 | 2) => Check::optional(
            "passwordless polkit authorization for DNS changes is not active (optional)",
        ),
        _ => {
            let error = String::from_utf8_lossy(&output.stderr);
            Check::warning(
                format!(
                    "polkit authorization could not be checked: {}",
                    error.trim()
                ),
                "Verify that polkit is running, then run `kvn-tui doctor` again.",
            )
        }
    }
}

/// Build the non-racy `PID,START_TIME,UID` identity recommended by pkcheck.
fn polkit_process_identity() -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    let pid = std::process::id();
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let start_time = proc_start_time(&stat)?;
    let uid = std::fs::metadata("/proc/self").ok()?.uid();
    Some(format!("{pid},{start_time},{uid}"))
}

fn proc_start_time(stat: &str) -> Option<&str> {
    // `/proc/<pid>/stat` fields 2 and 3 are `(comm)` and state. The command
    // may contain spaces or parentheses, so split only after its final `)`.
    // starttime is field 22, i.e. token 19 when counting from field 3.
    stat.rsplit_once(") ")?.1.split_whitespace().nth(19)
}

fn check_omarchy() -> Check {
    match crate::omarchy::detect_omarchy_theme() {
        Some(theme) => Check::pass(format!("Omarchy detected; active theme: {theme}")),
        None => Check::optional("Omarchy was not detected (optional)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parses_release_and_prerelease_versions() {
        assert_eq!(parse_version("sing-box version 1.13.15"), Some((1, 13, 15)));
        assert_eq!(parse_version("sing-box v1.12.0-beta.1"), Some((1, 12, 0)));
    }

    #[test]
    fn rejects_output_without_semver() {
        assert_eq!(parse_version("sing-box development build"), None);
    }

    #[test]
    fn version_comparison_rejects_old_releases() {
        assert!((1, 11, 9) < MIN_SINGBOX_VERSION);
        assert!((1, 12, 0) >= MIN_SINGBOX_VERSION);
    }

    #[test]
    fn executable_file_checks_execute_bits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        std::fs::write(&path, b"tool").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!executable_file(&path));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(executable_file(&path));
    }

    #[test]
    fn failure_check_has_remedy() {
        let check = Check::failure("broken", "fix it");
        assert_eq!(check.level, Level::Failure);
        assert_eq!(check.remedy.as_deref(), Some("fix it"));
    }

    #[test]
    fn optional_check_does_not_have_remedy() {
        let check = Check::optional("not installed");
        assert_eq!(check.level, Level::Optional);
        assert!(check.remedy.is_none());
    }

    #[test]
    fn parses_start_time_from_proc_stat() {
        let stat = "123 (kvn tui) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 424242 20";
        assert_eq!(proc_start_time(stat), Some("424242"));
    }

    #[test]
    fn rejects_malformed_proc_stat() {
        assert_eq!(proc_start_time("not proc stat"), None);
    }
}
