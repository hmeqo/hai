use clap::Parser;
use hai::{cli::Cli, config::AppConfigManager};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfigManager::from_file("hai.toml")?.with_env("HAI")?;

    cli.execute(config).await?;

    Ok(())
}
