use crate::config::{AppConfig, Config};
use crate::coordinator::Coordinator;
use clap::{Parser, Subcommand};
use serde_json;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print the loaded configuration
    Config,
    /// Run the application
    Run,
}

impl Cli {
    pub async fn execute(self, config: Config<AppConfig>) -> anyhow::Result<()> {
        match self.command {
            Commands::Config => {
                let cfg = config.load();
                println!("{}", serde_json::to_string_pretty(&*cfg)?);
            }
            Commands::Run => {
                let cfg = config.load();

                tracing_subscriber::FmtSubscriber::builder()
                    .with_max_level(cfg.logging.level())
                    .init();

                Coordinator::new(config).run().await?;
            }
        }
        Ok(())
    }
}
