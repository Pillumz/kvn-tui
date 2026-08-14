use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};

use crate::services::waybar;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, help = "Print connection status as JSON for Waybar integration")]
    waybar_status: bool,

    #[arg(long, help = "Run the headless daemon that manages sing-box")]
    pub daemon: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether kvn-tui and its runtime dependencies are ready.
    Doctor,

    /// Set up one or more optional kvn-tui integrations.
    #[command(group(
        ArgGroup::new("targets")
            .required(true)
            .multiple(true)
            .args(["omarchy", "polkit", "killswitch"])
    ))]
    Setup {
        /// Set up Omarchy integration (Waybar module, launcher, and Hyprland keybinding).
        #[arg(long)]
        omarchy: bool,

        /// Set up polkit access for passwordless DNS management.
        #[arg(long)]
        polkit: bool,

        /// Set up the nftables-based kill switch.
        #[arg(long)]
        killswitch: bool,
    },

    /// Remove files left behind by optional integration setup.
    #[command(group(
        ArgGroup::new("targets")
            .required(true)
            .multiple(true)
            .args(["omarchy"])
    ))]
    Clean {
        /// Remove backup files created by `setup --omarchy`.
        #[arg(long)]
        omarchy: bool,
    },
}

/// Run the embedded Omarchy integration installer script.
fn install_omarchy() -> Result<()> {
    let script = include_str!("../contrib/install-omarchy.sh");
    let tmp = std::env::temp_dir().join("kvn-tui-install-omarchy.sh");
    std::fs::write(&tmp, script)?;
    let status = std::process::Command::new("bash").arg(&tmp).status()?;
    std::fs::remove_file(&tmp).ok();
    if !status.success() {
        anyhow::bail!("install-omarchy.sh exited with status {}", status);
    }
    Ok(())
}

/// Run the embedded Omarchy integration cleanup script.
fn clean_omarchy() -> Result<()> {
    let script = include_str!("../contrib/clean-omarchy.sh");
    let tmp = std::env::temp_dir().join("kvn-tui-clean-omarchy.sh");
    std::fs::write(&tmp, script)?;
    let status = std::process::Command::new("bash")
        .arg(&tmp)
        .status()
        .context("failed to run clean-omarchy.sh")?;
    std::fs::remove_file(&tmp).ok();
    if !status.success() {
        anyhow::bail!("clean-omarchy.sh exited with status {}", status);
    }
    Ok(())
}

/// Run the embedded polkit rule installer script.
fn install_polkit() -> Result<()> {
    let script = include_str!("../contrib/install-polkit.sh");
    let tmp = std::env::temp_dir().join("kvn-tui-install-polkit.sh");
    std::fs::write(&tmp, script)?;
    let status = std::process::Command::new("bash")
        .arg(&tmp)
        .status()
        .context("failed to run install-polkit.sh")?;
    std::fs::remove_file(&tmp).ok();
    if !status.success() {
        anyhow::bail!("install-polkit.sh exited with status {}", status);
    }
    Ok(())
}

/// Run the embedded kill switch installer script.
fn install_killswitch() -> Result<()> {
    let script = include_str!("../contrib/install-killswitch.sh");
    let tmp = std::env::temp_dir().join("kvn-tui-install-killswitch.sh");
    std::fs::write(&tmp, script)?;
    let status = std::process::Command::new("bash")
        .arg(&tmp)
        .status()
        .context("failed to run install-killswitch.sh")?;
    std::fs::remove_file(&tmp).ok();
    if !status.success() {
        anyhow::bail!("install-killswitch.sh exited with status {}", status);
    }
    Ok(())
}

/// Parse CLI arguments and execute any non-TUI commands.
///
/// Returns `Some(Ok(()))` or `Some(Err(_))` if a CLI action was handled
/// and the application should exit. Returns `None` if the TUI should start.
#[allow(dead_code)]
pub fn try_run() -> Option<Result<()>> {
    let cli = Cli::parse();
    try_run_from_parsed(&cli)
}

/// Same as `try_run` but takes an already-parsed `Cli`.
pub fn try_run_from_parsed(cli: &Cli) -> Option<Result<()>> {
    match &cli.command {
        Some(Command::Doctor) => return Some(crate::doctor::run()),
        Some(Command::Setup {
            omarchy,
            polkit,
            killswitch,
        }) => {
            let result = (|| {
                if *omarchy {
                    install_omarchy()?;
                }
                if *polkit {
                    install_polkit()?;
                }
                if *killswitch {
                    install_killswitch()?;
                }
                Ok(())
            })();
            return Some(result);
        }
        Some(Command::Clean { omarchy }) => {
            let result = (|| {
                if *omarchy {
                    clean_omarchy()?;
                }
                Ok(())
            })();
            return Some(result);
        }
        None => {}
    }
    if cli.waybar_status {
        waybar::print_status();
        return Some(Ok(()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_via_clap() {
        // clap handles --version automatically
        let cli = Cli::try_parse_from(["kvn-tui", "--version"]);
        assert!(cli.is_err()); // clap exits on --version, but in test it returns Err
    }

    #[test]
    fn waybar_status_flag_detected() {
        let cli = Cli::parse_from(["kvn-tui", "--waybar-status"]);
        assert!(cli.waybar_status);
    }

    #[test]
    fn setup_omarchy_option_detected() {
        let cli = Cli::parse_from(["kvn-tui", "setup", "--omarchy"]);
        assert!(matches!(
            cli.command,
            Some(Command::Setup {
                omarchy: true,
                polkit: false,
                killswitch: false,
            })
        ));
    }

    #[test]
    fn setup_polkit_option_detected() {
        let cli = Cli::parse_from(["kvn-tui", "setup", "--polkit"]);
        assert!(matches!(
            cli.command,
            Some(Command::Setup {
                omarchy: false,
                polkit: true,
                killswitch: false,
            })
        ));
    }

    #[test]
    fn setup_killswitch_option_detected() {
        let cli = Cli::parse_from(["kvn-tui", "setup", "--killswitch"]);
        assert!(matches!(
            cli.command,
            Some(Command::Setup {
                omarchy: false,
                polkit: false,
                killswitch: true,
            })
        ));
    }

    #[test]
    fn setup_options_can_be_combined() {
        let cli = Cli::parse_from(["kvn-tui", "setup", "--omarchy", "--polkit", "--killswitch"]);
        assert!(matches!(
            cli.command,
            Some(Command::Setup {
                omarchy: true,
                polkit: true,
                killswitch: true,
            })
        ));
    }

    #[test]
    fn setup_requires_at_least_one_option() {
        assert!(Cli::try_parse_from(["kvn-tui", "setup"]).is_err());
    }

    #[test]
    fn clean_omarchy_option_detected() {
        let cli = Cli::parse_from(["kvn-tui", "clean", "--omarchy"]);
        assert!(matches!(
            cli.command,
            Some(Command::Clean { omarchy: true })
        ));
    }

    #[test]
    fn clean_requires_at_least_one_option() {
        assert!(Cli::try_parse_from(["kvn-tui", "clean"]).is_err());
    }

    #[test]
    fn daemon_flag_detected() {
        let cli = Cli::parse_from(["kvn-tui", "--daemon"]);
        assert!(cli.daemon);
    }

    #[test]
    fn doctor_subcommand_detected() {
        let cli = Cli::parse_from(["kvn-tui", "doctor"]);
        assert!(matches!(cli.command, Some(Command::Doctor)));
    }
}
