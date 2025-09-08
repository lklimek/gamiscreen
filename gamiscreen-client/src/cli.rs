use std::path::PathBuf;

use crate::platform::linux::lock::LockMethod;
use clap::{Parser, Subcommand};

const HELP_EPILOG: &str = r#"Config resolution order:
  1) --config/-c PATH
  2) $GAMISCREEN_CONFIG
  3) XDG default: ~/.config/gamiscreen/client.yaml
"#;

#[derive(Debug, Parser)]
#[command(
    name = "gamiscreen-client",
    version,
    about = "Linux client session agent for GamiScreen",
    long_about = None,
    after_long_help = HELP_EPILOG,
)]
pub struct Cli {
    /// Path to YAML config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Optional subcommand. Without one, runs the agent.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Log in to the server and save token in the keyring
    Login {
        /// Server URL (e.g., http://127.0.0.1:5151). Falls back to config or prompt.
        #[arg(long)]
        server: Option<String>,
        /// Username. Falls back to prompt.
        #[arg(long)]
        username: Option<String>,
    },
    /// Install helper: polkit rule + user systemd service
    ///
    /// When run as root, you must provide --user (or you will be prompted).
    Install {
        /// Target username for user-level setup (systemd --user, unit location)
        #[arg(long)]
        user: Option<String>,
    },
    /// Uninstall helper: remove polkit rule and user systemd service
    ///
    /// When run as root, you must provide --user (or you will be prompted).
    Uninstall {
        /// Target username for user-level cleanup
        #[arg(long)]
        user: Option<String>,
    },
    /// Try lock methods and report status
    Lock {
        /// Method to use (default: all)
        #[arg(long, value_enum, default_value_t = LockMethod::All)]
        method: LockMethod,
    },
}
