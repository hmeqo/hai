use clap::Parser;
use hai::{cli::Cli, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    Cli::parse().execute().await
}
