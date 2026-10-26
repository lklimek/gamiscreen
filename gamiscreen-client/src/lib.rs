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

pub(crate) struct LogConfig {
    pub dir: std::path::PathBuf,
    pub prefix: &'static str,
}

fn init_tracing(file_log: Option<LogConfig>) {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .compact()
        .with_filter(tracing_subscriber::EnvFilter::new(&env_filter));

    let file_layer = file_log.and_then(|cfg| {
        if let Err(e) = std::fs::create_dir_all(&cfg.dir) {
            eprintln!("Warning: cannot create log dir {}: {e}", cfg.dir.display());
            return None;
        }
        match tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(7)
            .filename_prefix(cfg.prefix)
            .build(&cfg.dir)
        {
            Ok(appender) => Some(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_ansi(false)
                    .with_writer(appender)
                    .with_filter(tracing_subscriber::EnvFilter::new(&env_filter)),
            ),
            Err(e) => {
                eprintln!("Warning: cannot create file appender: {e}");
                None
            }
        }
    });

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();
}

fn resolve_log_config(command: &Command) -> Option<LogConfig> {
    match command {
        #[cfg(target_os = "windows")]
        Command::Service(cli::ServiceCommand::Run) => {
            let program_data =
                std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
            Some(LogConfig {
                dir: std::path::PathBuf::from(program_data).join(r"gamiscreen\logs"),
                prefix: "service",
            })
        }
        #[cfg(target_os = "windows")]
        Command::SessionAgent { .. } => {
            let dir =
                directories::ProjectDirs::from("ws.klimek.gamiscreen", "gamiscreen", "gamiscreen")
                    .map(|pd| pd.data_local_dir().join("logs"))
                    .unwrap_or_else(|| {
                        let local = std::env::var("LOCALAPPDATA")
                            .unwrap_or_else(|_| r"C:\Users\Public\AppData\Local".to_string());
                        std::path::PathBuf::from(local).join(r"gamiscreen\gamiscreen\logs")
                    });
            Some(LogConfig {
                dir,
                prefix: "agent",
            })
        }
        _ => None,
    }
}

pub(crate) fn keyring_entry(server_url: &str) -> Result<keyring::Entry, AppError> {
    let service = "gamiscreen-client";
    keyring::Entry::new(service, &crate::config::normalize_server_url(server_url))
        .map_err(|e| AppError::Keyring(e.to_string()))
}

pub async fn run(cli: Cli) -> Result<(), AppError> {
    let Cli { config, command } = cli;
    let command = command.unwrap_or(Command::Agent);
    let log_config = resolve_log_config(&command);
    init_tracing(log_config);

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
        Command::SessionAgent { session_id } => {
            tracing::info!(session_id, "starting session agent");
            platform::windows::util::verify_session_id(session_id)?;
            app::agent::run(config.clone()).await
        }
        #[cfg(not(target_os = "windows"))]
        Command::Lock { method } => {
            platform::linux::lock_tester::run_lock_cmd(method).await;
            Ok(())
        }
    }
}
