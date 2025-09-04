use clap::{Parser, Subcommand};
use std::path::PathBuf;

const HELP_EPILOG: &str = r#"Server options can also be provided via environment variables:
  CONFIG_PATH (default: ./config.yaml)
  DB_PATH     (default: data/app.db)
  PORT        (default: 5151 or config.listen_port)

The `install` command helps set up a systemd service and a default config.
Run it as root (or with sudo) for system-wide install.
"#;

#[derive(Debug, Parser)]
#[command(
    name = "gamiscreen-server",
    version,
    about = "GamiScreen server",
    long_about = None,
    after_long_help = HELP_EPILOG,
)]
pub struct Cli {
    /// Optional subcommand. Without one, runs the server.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install systemd unit + default config (run as root for system-wide)
    Install {
        /// Destination path for systemd unit
        #[arg(long, default_value = "/etc/systemd/system/gamiscreen-server.service")]
        unit_path: PathBuf,
        /// Destination path for server config
        #[arg(long, default_value = "/etc/gamiscreen/config.yaml")]
        config_path: PathBuf,
        /// Default DB path to include in unit env
        #[arg(long, default_value = "/var/lib/gamiscreen/app.db")]
        db_path: PathBuf,
        /// Absolute path to the server binary used in ExecStart
        #[arg(long)]
        bin_path: Option<PathBuf>,
        /// systemd service user (defaults to 'gamiscreen')
        #[arg(long, default_value = "gamiscreen")]
        user: String,
        /// systemd service group (defaults to 'gamiscreen')
        #[arg(long, default_value = "gamiscreen")]
        group: String,
        /// Working directory for the service (defaults to /var/lib/gamiscreen)
        #[arg(long, default_value = "/var/lib/gamiscreen")]
        working_dir: PathBuf,
        /// Overwrite files if they already exist
        #[arg(long)]
        force: bool,
    },
    /// Uninstall systemd unit; optionally remove config
    Uninstall {
        /// Path to systemd unit to remove
        #[arg(long, default_value = "/etc/systemd/system/gamiscreen-server.service")]
        unit_path: PathBuf,
        /// Also remove config file
        #[arg(long)]
        remove_config: bool,
        /// Path to config file
        #[arg(long, default_value = "/etc/gamiscreen/config.yaml")]
        config_path: PathBuf,
    },
}
