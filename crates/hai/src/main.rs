use clap::Parser;
use hai::cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().execute().await
}
