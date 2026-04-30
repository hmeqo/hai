use clap::Parser;
use hai::{cli::Cli, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    // hai::minimal::run().await
    Cli::parse().execute().await
}
