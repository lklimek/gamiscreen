use clap::Parser;
use gamiscreen_client::{Cli, run};

#[tokio::main]
async fn main() -> Result<(), gamiscreen_client::AppError> {
    run(Cli::parse()).await
}
