use clap::Args;

use crate::{
    config::{AppConfigManager, PathResolver, env::ENV_PREFIX},
    domain::db,
};

#[derive(Args)]
pub struct LogArgs {
    /// Show detail for a specific event seq
    #[arg(long)]
    pub id: Option<i64>,

    /// Filter by chat id
    #[arg(long)]
    pub chat: Option<i64>,

    /// Filter by event type (turn_started, tool_call, etc.)
    #[arg(long)]
    pub event: Option<String>,
}

pub async fn execute(args: LogArgs) -> crate::error::Result<()> {
    let config = AppConfigManager::from_file(PathResolver::config_file().to_str().unwrap())?
        .with_env(ENV_PREFIX)?;
    let cfg = config.load();
    let (db, _pool) = db::init_db(&cfg.database).await?;

    if let Some(seq) = args.id {
        show_detail(seq, &db).await?;
    } else {
        let mut app = super::tui::TuiApp::new(db);
        app.set_chat_filter(args.chat);
        app.set_kind_filter(args.event);
        app.run()
            .await
            .map_err(|e| crate::error::ErrorKind::Internal.msg(e.to_string()))?;
    }

    Ok(())
}

async fn show_detail(seq: i64, db: &toasty::Db) -> crate::error::Result<()> {
    use super::display::{self, EventDisplay};

    let Some(event) = display::query_event_by_seq(db, seq).await? else {
        eprintln!("Event #{seq} not found");
        return Ok(());
    };

    let d = EventDisplay::from_event(&event);
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
