use gamiscreen_server::{server, storage};
mod cli;
mod install;

use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    use clap::Parser;
    let args = cli::Cli::parse();
    if let Some(cmd) = args.command {
        match cmd {
            cli::Command::Install {
                unit_path,
                config_path,
                db_path,
                bin_path,
                user,
                group,
                working_dir,
                force,
            } => {
                let bin = bin_path.unwrap_or_else(|| {
                    std::env::current_exe().unwrap_or_else(|_| {
                        std::path::PathBuf::from("/usr/local/bin/gamiscreen-server")
                    })
                });
                if let Err(e) = install::install_system(
                    &unit_path,
                    &config_path,
                    &db_path,
                    &bin,
                    &user,
                    &group,
                    &working_dir,
                    force,
                ) {
                    eprintln!("Install error: {}", e);
                    std::process::exit(2);
                }
                return;
            }
            cli::Command::Uninstall {
                unit_path,
                remove_config,
                config_path,
            } => {
                if let Err(e) = install::uninstall_system(&unit_path, remove_config, &config_path) {
                    eprintln!("Uninstall error: {}", e);
                    std::process::exit(2);
                }
                return;
            }
        }
    }
    // Console-only logging with env-driven level
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_ansi(true)
        .init();

    let config = match server::AppConfig::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error=%e, "Failed to load config");
            std::process::exit(2);
        }
    };

    // Connect storage (SQLite via SeaORM)
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "data/app.db".into());
    // Ensure data dir exists when using default
    if let Some(parent) = std::path::Path::new(&db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = match storage::Store::connect_sqlite(&db_path).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error=%e, path=%db_path, "Failed to connect DB");
            std::process::exit(3);
        }
    };

    // Seed children/tasks from config
    if let Err(e) = store
        .seed_from_config(&config.children, &config.tasks)
        .await
    {
        tracing::error!(error=%e, "Failed to seed DB");
        std::process::exit(4);
    }

    // Decide listen port: env PORT overrides config.listen_port, default 5151
    let port = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .or(config.listen_port)
        .unwrap_or(5151);

    let state = server::AppState::new(config, store);

    let app = server::router(state);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    tracing::info!(%addr, "Starting server");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind listener");

    if let Err(err) = axum::serve(listener, app).await {
        tracing::error!(%err, "server error");
    }
}
