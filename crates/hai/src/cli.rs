pub mod display;
pub mod log;
pub mod tui;

use clap::{Parser, Subcommand, ValueEnum, builder::Styles};
use serde::{Deserialize, Serialize};

use crate::{
    App,
    config::{AppConfigManager, PathResolver, env::ENV_PREFIX},
    domain::db,
    rebuild,
    util::pgvector,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None, styles = Styles::styled())]
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
    /// Database operations
    Db {
        #[command(subcommand)]
        action: DbAction,
    },
    /// View agent event log
    Log {
        #[command(flatten)]
        args: log::LogArgs,
    },
}

#[derive(Subcommand)]
pub enum DbAction {
    /// Create the database if it doesn't exist
    Create,
    /// Apply pending migrations
    Migrate,
    /// Rebuild vector embeddings using the current embedding model
    Rebuild {
        #[command(subcommand)]
        target: RebuildTarget,
    },
}

#[derive(Subcommand)]
pub enum RebuildTarget {
    /// Re-generate all vector embeddings using current model
    Embeddings,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFormat {
    Json,
    Toml,
}

impl Cli {
    pub async fn execute(self) -> crate::error::Result<()> {
        let config = AppConfigManager::from_file(PathResolver::config_file().to_str().unwrap())?
            .with_env(ENV_PREFIX)?;

        if let Some(command) = self.command {
            match command {
                Commands::Config { r#format } => {
                    let cfg = config.load();
                    match r#format {
                        ConfigFormat::Json => println!("{}", serde_json::to_string_pretty(&*cfg)?),
                        ConfigFormat::Toml => println!("{}", toml::to_string_pretty(&*cfg)?),
                    }
                }
                Commands::Db { action } => {
                    let cfg = config.load();
                    match action {
                        DbAction::Create => {
                            db::create_database(&cfg.database.url).await?;
                        }
                        DbAction::Migrate => {
                            let (mut db, _pool) = db::init_db(&cfg.database).await?;
                            db::run_migrations(&db).await?;
                            let dim = cfg.multimodal.embedding.dimension.unwrap_or(1024);
                            pgvector::ensure_embedding_schema(&mut db, dim).await?;
                        }
                        DbAction::Rebuild { target } => match target {
                            RebuildTarget::Embeddings => {
                                rebuild::rebuild_embeddings(&cfg).await?;
                            }
                        },
                    }
                }
                Commands::Log { args } => {
                    log::execute(args).await?;
                }
            }
        } else {
            App::serve(config).await?;
        }
        Ok(())
    }
}
