use clap::Parser;
use gamiscreen_client_linux::{Cli, run};

#[tokio::main]
async fn main() -> Result<(), gamiscreen_client_linux::AppError> {
    run(Cli::parse()).await
}
