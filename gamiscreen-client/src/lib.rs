pub mod app;
pub mod cli;
pub mod config;
pub mod login;
pub mod platform;
pub mod sse;
pub mod update;

pub use cli::{Cli, Command};
pub use config::{ClientConfig, load_config, resolve_config_path};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("dbus error: {0}")]
    Dbus(String),
    #[error("keyring error: {0}")]
    Keyring(String),
}

fn init_tracing() {
    let env_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

pub(crate) fn keyring_entry(server_url: &str) -> Result<keyring::Entry, AppError> {
    let service = "gamiscreen-client";
    keyring::Entry::new(service, &crate::config::normalize_server_url(server_url))
        .map_err(|e| AppError::Keyring(e.to_string()))
}

pub async fn run(cli: Cli) -> Result<(), AppError> {
    init_tracing();

    let Cli { config, command } = cli;
    let command = command.unwrap_or(Command::Agent);

    match command {
        Command::Agent => app::agent::run(config.clone()).await,
        Command::Login { server, username } => login::login(server, username, config.clone()).await,
        Command::Install { user } => {
            let plat = platform::detect_default().await?;
            plat.install(user).await
        }
        Command::Uninstall { user } => {
            let plat = platform::detect_default().await?;
            plat.uninstall(user).await
        }
        #[cfg(target_os = "windows")]
        Command::Service(action) => {
            platform::windows::service_cli::handle_service_command(action).await
        }
        #[cfg(target_os = "windows")]
        Command::SessionAgent => Err(AppError::Config(
            "session-agent command not implemented yet".into(),
        )),
        #[cfg(not(target_os = "windows"))]
        Command::Lock { method } => {
            platform::linux::lock_tester::run_lock_cmd(method).await;
            Ok(())
        }
    }
}
