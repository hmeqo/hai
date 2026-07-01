use std::path::Path;

use sqlx::PgPool;
use toasty_cli::ToastyCli;
use tracing::info;

use crate::{
    config::schema::DatabaseConfig,
    error::{ErrorKind, Result},
};

pub async fn init_db(config: &DatabaseConfig) -> Result<(toasty::Db, PgPool)> {
    let db = toasty::Db::builder()
        .models(toasty::models!(crate::*))
        .max_pool_size(config.max_connections as usize)
        .connect(&config.url)
        .await?;

    let pool = PgPool::connect(&config.url).await?;

    Ok((db, pool))
}

pub async fn run_migrations(db: &toasty::Db) -> Result<()> {
    let cli_config = toasty_cli::Config::load_or_default(Path::new("."))
        .map_err(|e| ErrorKind::Internal.msg(format!("Failed to load migration config: {e}")))?;
    let cli = ToastyCli::with_config(db.clone(), cli_config);
    cli.parse_from(["toasty", "migration", "apply"])
        .await
        .map_err(|e| ErrorKind::Internal.msg(format!("Migration apply failed: {e}")))?;
    Ok(())
}

pub async fn create_database(url: &str) -> Result<()> {
    let (admin_url, db_name) = split_admin_url(url);
    let mut admin = toasty::Db::builder().connect(&admin_url).await?;
    toasty::sql::statement(format!("CREATE DATABASE \"{db_name}\""))
        .exec(&mut admin)
        .await?;
    info!(%db_name, "Database created");
    Ok(())
}

fn split_admin_url(url: &str) -> (String, String) {
    let without_scheme = url.strip_prefix("postgresql://").unwrap_or(url);
    let last_slash = without_scheme.rfind('/').unwrap_or(without_scheme.len());
    let base = format!("postgresql://{}/postgres", &without_scheme[..last_slash]);
    let db_name = without_scheme[last_slash + 1..].to_string();
    (base, db_name)
}
