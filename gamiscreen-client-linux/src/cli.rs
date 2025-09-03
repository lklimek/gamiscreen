use std::path::PathBuf;

use clap::{Parser, Subcommand};

const HELP_EPILOG: &str = r#"Config resolution order:
  1) --config/-c PATH
  2) $GAMISCREEN_CONFIG
  3) XDG default: ~/.config/gamiscreen/client.yaml
"#;

#[derive(Debug, Parser)]
#[command(
    name = "gamiscreen-client-linux",
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
}
