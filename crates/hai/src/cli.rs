pub mod display;
pub mod kb;
pub mod log;
pub mod tui;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, builder::Styles};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    App,
    config::{AppConfigManager, Paths, env::ENV_PREFIX},
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
        #[clap(long, help = "Output format: json or toml", default_value = "json")]
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
    /// Knowledge base management
    Kb {
        #[command(subcommand)]
        action: KbAction,
    },
}

#[derive(Subcommand)]
pub enum KbAction {
    /// Import documents into the knowledge base (idempotent by content hash)
    Import {
        /// File or directory to import
        paths: Vec<PathBuf>,
        /// Collection label (empty = uncategorized)
        #[arg(long)]
        collection: Option<String>,
        /// Override document title (default: frontmatter title or file name)
        #[arg(long)]
        title: Option<String>,
        /// Recurse into directories
        #[arg(long)]
        recursive: bool,
    },
    /// List documents (optionally filtered by collection)
    List {
        #[arg(long)]
        collection: Option<String>,
    },
    /// Semantic search over the knowledge base
    Search {
        query: String,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: i64,
    },
    /// Delete a document (cascades chunks)
    Delete { id: Uuid },
    /// Re-chunk and re-embed documents whose chunker version is stale
    Reindex {
        #[arg(long)]
        collection: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum DbAction {
    /// Create the database
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
        let config = AppConfigManager::from_file(Paths::inferred().config_file_str())?
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
                            let pool = db::init_db(&cfg.database).await?;
                            db::run_migrations(&pool).await?;
                            let dim = cfg
                                .auxiliary
                                .embedding
                                .as_ref()
                                .map(|b| b.dimension())
                                .unwrap_or(1024);
                            pgvector::ensure_embedding_schema(&pool, dim).await?;
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
                Commands::Kb { action } => {
                    let cfg = config.load();
                    kb::execute(action, &cfg).await?;
                }
            }
        } else {
            App::serve(config).await?;
        }
        Ok(())
    }
}
