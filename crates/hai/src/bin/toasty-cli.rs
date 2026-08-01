use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    let cfg =
        hai::config::AppConfigManager::from_file(hai::config::Paths::inferred().config_file_str())?
            .with_env(hai::config::env::ENV_PREFIX)?
            .load();

    let cli_config = toasty_cli::Config::load()?;

    let db = toasty::Db::builder()
        .models(toasty::models!(hai::*))
        .connect(&cfg.database.url)
        .await?;

    let cli = toasty_cli::ToastyCli::with_config(db, cli_config);
    cli.parse_and_run().await?;

    Ok(())
}
