use std::str::FromStr;

use sqlx::PgPool;
use tracing::info;

use crate::{config::schema::DatabaseConfig, error::Result};

pub async fn init_db(config: &DatabaseConfig) -> Result<PgPool> {
    PgPool::connect_with(
        sqlx::postgres::PgConnectOptions::from_str(&config.url)
            .map_err(|e| crate::error::ErrorKind::Config.msg(e.to_string()))?,
    )
    .await
    .map_err(Into::into)
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    // 目标 schema（幂等建表——已部署库表已存在则跳过）
    sqlx::raw_sql(include_str!("../../migrations/schema.sql"))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn create_database(url: &str) -> Result<()> {
    let (admin_url, db_name) = split_admin_url(url);
    let pool = PgPool::connect(&admin_url).await?;
    let sql = format!("CREATE DATABASE \"{db_name}\"");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .execute(&pool)
        .await?;
    info!(%db_name, "Database created");
    Ok(())
}

fn split_admin_url(url: &str) -> (String, String) {
    let stripped = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))
        .unwrap_or(url);
    let (before_query, query) = stripped.split_once('?').unwrap_or((stripped, ""));
    let last_slash = before_query.rfind('/').unwrap_or(before_query.len());
    let mut admin = format!("postgresql://{}/postgres", &before_query[..last_slash]);
    if !query.is_empty() {
        admin.push('?');
        admin.push_str(query);
    }
    let db_name = before_query[last_slash + 1..].to_string();
    (admin, db_name)
}
