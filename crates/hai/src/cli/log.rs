use clap::Args;

use crate::{
    config::{AppConfigManager, Paths, env::ENV_PREFIX},
    domain::{db, repo::Repos},
};

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
}

pub async fn execute(args: LogArgs) -> crate::error::Result<()> {
    let config =
        AppConfigManager::from_file(Paths::inferred().config_file_str())?.with_env(ENV_PREFIX)?;
    let cfg = config.load();
    let pool = db::init_db(&cfg.database).await?;
    let repos = Repos::new(pool);

    if let Some(seq) = args.id {
        show_detail(seq, &repos).await?;
    } else {
        super::tui::run_tui(repos, args.chat, args.event).await?;
    }

    Ok(())
}

async fn show_detail(seq: i64, repos: &Repos) -> crate::error::Result<()> {
    use super::display::{self, EventDisplay};

    let Some(event) = repos.event.by_seq(seq).await? else {
        eprintln!("Event #{seq} not found");
        return Ok(());
    };

    let Some(d) = EventDisplay::from_event(&event) else {
        eprintln!("Event #{seq}: unable to deserialize");
        return Ok(());
    };
    println!(
        "{}  {}  {}",
        display::fmt_time(event.created_at),
        display::chat_display(&event),
        d.tag
    );
    println!("────────────────────────────────────────────────────────────────────────");
    println!("{}", d.detail_text);

    Ok(())
}
