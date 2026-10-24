pub mod display;
pub mod kb;
pub mod log;
pub(crate) mod rebuild;
pub mod tui;

use std::{path::PathBuf, sync::Arc};

use clap::{Args, Parser, Subcommand, ValueEnum, builder::Styles};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    bootstrap,
    config::{AppConfig, Paths, env},
    domain::{
        repo::Repos,
        service::ConversationService,
        storage,
        vo::{ChatId, ConversationId},
    },
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
        args: LogArgs,
    },
    /// Knowledge base management
    Kb {
        #[command(subcommand)]
        action: KbAction,
    },
    /// Inspect or change Conversation membership
    Conversation {
        #[command(subcommand)]
        action: ConversationAction,
    },
}

#[derive(Subcommand)]
pub enum ConversationAction {
    /// Move one Chat into an existing Conversation, carrying its pending entries
    Attach {
        #[arg(long)]
        chat: i64,
        #[arg(long)]
        conversation: i64,
    },
    /// Move one Chat into a fresh Conversation, carrying its pending entries
    Detach {
        #[arg(long)]
        chat: i64,
    },
    /// List Conversations and their member Chats
    List,
}

#[derive(Args)]
pub struct LogArgs {
    /// Show detail for a specific event seq
    #[arg(long)]
    pub id: Option<i64>,

    /// Filter by chat id
    #[arg(long)]
    pub chat: Option<i64>,

    /// Filter by event type (kebab-case: turn-started, tool-call, turn-ended, etc.)
    #[arg(long)]
    pub event: Option<String>,

    /// Filter by session id prefix
    #[arg(long)]
    pub session: Option<String>,
}

#[derive(Subcommand)]
pub enum KbAction {
    /// Import documents into the knowledge base (idempotent by content hash)
    Import {
        /// File or directory to import
        paths: Vec<PathBuf>,
        /// Collection label (empty = uncategorised)
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
    /// Create every table, index and constraint the schema declares, at the configured embedding
    /// dimension
    Migrate,
    /// Rebuild vector embeddings using the current embedding model
    Rebuild {
        #[command(subcommand)]
        target: RebuildTarget,
    },
}

#[derive(Subcommand)]
pub enum RebuildTarget {
    /// Re-embed every row with the current embedding model
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
        let paths = Paths::inferred();
        env::load_dotenv(&paths.env_file())?;

        let cfg = Arc::new(AppConfig::load(paths.config_file())?);

        if let Some(command) = self.command {
            match command {
                Commands::Config { r#format } => match r#format {
                    ConfigFormat::Json => println!("{}", serde_json::to_string_pretty(&*cfg)?),
                    ConfigFormat::Toml => println!("{}", toml::to_string_pretty(&*cfg)?),
                },
                Commands::Db { action } => match action {
                    DbAction::Create => {
                        storage::create_database(&cfg.database.url).await?;
                    }
                    DbAction::Migrate => {
                        storage::migrate(&cfg).await?;
                    }
                    DbAction::Rebuild { target } => match target {
                        RebuildTarget::Embeddings => {
                            rebuild::rebuild_embeddings(&cfg).await?;
                        }
                    },
                },
                Commands::Log { args } => {
                    log::execute(args, &cfg).await?;
                }
                Commands::Kb { action } => {
                    kb::execute(action, &cfg).await?;
                }
                Commands::Conversation { action } => {
                    let pool = sqlx::PgPool::connect(&cfg.database.url).await?;
                    let service = ConversationService::new(Repos::new(pool));
                    match action {
                        ConversationAction::Attach { chat, conversation } => {
                            service
                                .attach(ChatId(chat), ConversationId(conversation))
                                .await?;
                            println!("Chat {chat} moved to Conversation {conversation}");
                        }
                        ConversationAction::Detach { chat } => {
                            let conversation = service.detach(ChatId(chat)).await?;
                            println!("Chat {chat} moved to Conversation {conversation}");
                        }
                        ConversationAction::List => {
                            for conversation in service.list().await? {
                                println!("conversation {}", conversation.id);
                                for chat in conversation.chats {
                                    println!(
                                        "  chat {} alias={} channel={} external={}",
                                        chat.id, chat.alias, chat.channel_key, chat.external_id
                                    );
                                }
                            }
                        }
                    }
                }
            }
        } else {
            bootstrap::serve(cfg).await?;
        }
        Ok(())
    }
}
