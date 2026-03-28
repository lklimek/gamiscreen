use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[cfg(not(target_os = "windows"))]
use crate::platform::linux::lock::LockMethod;

// Note: ProjectDirs::from("dev", "gamiscreen", "gamiscreen") on Windows
// resolves to %APPDATA%\gamiscreen\gamiscreen\config\. The help text below
// shows a simplified path for readability. The actual path is determined
// at runtime by the `directories` crate.
#[cfg(target_os = "windows")]
const HELP_EPILOG: &str = r#"Config resolution order:
  1) --config/-c PATH
  2) %GAMISCREEN_CONFIG%
  3) Default: %APPDATA%\gamiscreen\client.yaml
"#;

#[cfg(not(target_os = "windows"))]
const HELP_EPILOG: &str = r#"Config resolution order:
  1) --config/-c PATH
  2) $GAMISCREEN_CONFIG
  3) XDG default: ~/.config/gamiscreen/client.yaml
"#;

#[derive(Debug, Parser)]
#[command(
    name = "gamiscreen-client",
    version,
    about = "Client utilities and agents for GamiScreen",
    long_about = r"Run the per-session agent (default) or manage platform-specific installs. On Windows, use the `service` commands for system-wide installs.",
    after_long_help = HELP_EPILOG,
)]
pub struct Cli {
    /// Path to YAML config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Optional subcommand. Defaults to `agent` when omitted.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the interactive agent in the current session
    Agent,
    /// Log in to the server and save token in the keyring
    Login {
        /// Server URL (e.g., https://your-server.example or http://127.0.0.1:5151). Falls back to config or prompt.
        #[arg(long)]
        server: Option<String>,
        /// Username. Falls back to prompt.
        #[arg(long)]
        username: Option<String>,
    },
    /// Install background agent/service for this platform
    ///
    /// Linux: polkit rule + user systemd unit. When run as root, provide --user (or you will be prompted).
    /// Windows: delegates to `gamiscreen-client service install`. Prefer using the `service` subcommand directly.
    Install {
        /// Target username (Linux root only). Ignored on Windows.
        #[arg(long)]
        user: Option<String>,
    },
    /// Uninstall background agent/service for this platform
    ///
    /// Linux: removes polkit rule + user systemd unit.
    /// Windows: delegates to `gamiscreen-client service uninstall`. Prefer using the `service` subcommand directly.
    Uninstall {
        /// Target username (Linux root only). Ignored on Windows.
        #[arg(long)]
        user: Option<String>,
    },
    #[cfg(target_os = "windows")]
    /// Windows service management commands
    #[command(subcommand)]
    Service(ServiceCommand),
    #[cfg(target_os = "windows")]
    /// Run the Windows session agent worker (spawned by the service)
    SessionAgent {
        /// Windows session ID this agent is running in
        #[arg(long)]
        session_id: u32,
    },
    #[cfg(not(target_os = "windows"))]
    /// Try lock methods and report status
    Lock {
        /// Method to use (default: all)
        #[arg(long, value_enum, default_value_t = LockMethod::All)]
        method: LockMethod,
    },
}

#[cfg(target_os = "windows")]
#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Install the Windows service
    Install,
    /// Remove the Windows service
    Uninstall,
    /// Start the Windows service via SCM
    Start,
    /// Stop the Windows service via SCM
    Stop,
    /// Run the service host (invoked by SCM)
    Run,
}
