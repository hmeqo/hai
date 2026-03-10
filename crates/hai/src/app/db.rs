use anyhow::Result;
use sqlx::PgPool;
use sqlx::migrate::MigrateDatabase;
use sqlx::postgres::PgPoolOptions;

use crate::config::schema::DatabaseConfig;

/// 初始化 PostgreSQL 连接池并运行数据库迁移
pub async fn init_pool(config: &DatabaseConfig) -> Result<PgPool> {
    if !sqlx::Postgres::database_exists(&config.url).await? {
        sqlx::Postgres::create_database(&config.url).await?;
    }

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.url)
        .await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    Ok(pool)
}
