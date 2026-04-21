use crate::{
    App,
    config::{AppConfigManager, PathResolver},
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print the loaded configuration
    Config {
        #[clap(long, help = "Output format: json or yaml", default_value = "json")]
        r#format: ConfigFormat,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFormat {
    Json,
    Toml,
}

impl Cli {
    pub async fn execute(self) -> anyhow::Result<()> {
        let config = AppConfigManager::from_file(PathResolver::config_file().to_str().unwrap())?
            .with_env("HAI")?;

        if let Some(command) = self.command {
            match command {
                Commands::Config { r#format } => {
                    let cfg = config.load();
                    match r#format {
                        ConfigFormat::Json => println!("{}", serde_json::to_string_pretty(&*cfg)?),
                        ConfigFormat::Toml => println!("{}", toml::to_string_pretty(&*cfg)?),
                    }
                }
            }
        } else {
            App::serve(config).await?;
        }
        Ok(())
    }
}
