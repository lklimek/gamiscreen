use tracing::{info, warn};

use super::service;
use crate::AppError;
use crate::cli::ServiceCommand;

/// Handle Windows service management commands.
pub async fn handle_service_command(action: ServiceCommand) -> Result<(), AppError> {
    match action {
        ServiceCommand::Run => {
            info!("Windows: service run command invoked");
            let result = tokio::task::block_in_place(|| service::run_service_host());
            result
        }
        ServiceCommand::Install => {
            warn!("Windows: service install command not implemented yet");
            Err(AppError::Config(
                "service install command not implemented yet".into(),
            ))
        }
        ServiceCommand::Uninstall => {
            warn!("Windows: service uninstall command not implemented yet");
            Err(AppError::Config(
                "service uninstall command not implemented yet".into(),
            ))
        }
        ServiceCommand::Start => {
            warn!("Windows: service start command not implemented yet");
            Err(AppError::Config(
                "service start command not implemented yet".into(),
            ))
        }
        ServiceCommand::Stop => {
            warn!("Windows: service stop command not implemented yet");
            Err(AppError::Config(
                "service stop command not implemented yet".into(),
            ))
        }
    }
}
