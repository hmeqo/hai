use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::model::Memory, error::Result};

#[derive(Debug, Clone)]
pub struct MemoryRepo {
    pool: PgPool,
}

impl MemoryRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 查重：kind + chat_id + content 命中即视为已存在（account_id 提供时追加匹配）。
    pub async fn find_duplicate(
        &self,
        kind: &str,
        chat_id: i64,
        content: &str,
        account_id: Option<i64>,
    ) -> Result<Option<Memory>> {
        let mut sql =
            String::from("SELECT * FROM memory WHERE kind = $1 AND chat_id = $2 AND content = $3");
        if account_id.is_some() {
            sql.push_str(" AND account_id = $4");
        }
        sql.push_str(" LIMIT 1");
        let mut q = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(kind)
            .bind(chat_id)
            .bind(content);
        if let Some(aid) = account_id {
            q = q.bind(aid);
        }
        q.fetch_optional(&self.pool).await.map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        id: Uuid,
        kind: &str,
        chat_id: i64,
        account_id: Option<i64>,
        content: &str,
        importance: i32,
        meta: Option<Value>,
        created_at: jiff::Timestamp,
        updated_at: jiff::Timestamp,
    ) -> Result<Memory> {
        sqlx::query_as::<_, Memory>(
            "INSERT INTO memory (id, account_id, chat_id, kind, content, importance, meta, \
             created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING *",
        )
        .bind(id)
        .bind(account_id)
        .bind(chat_id)
        .bind(kind)
        .bind(content)
        .bind(importance)
        .bind(meta)
        .bind(jiff_sqlx::Timestamp::from(created_at))
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Memory>> {
        sqlx::query_as::<_, Memory>("SELECT * FROM memory WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// 按需更新 content / importance（仅更新提供的字段）。
    pub async fn update_fields(
        &self,
        id: Uuid,
        content: Option<&str>,
        importance: Option<i32>,
    ) -> Result<()> {
        if content.is_none() && importance.is_none() {
            return Ok(());
        }
        let mut sets: Vec<String> = Vec::with_capacity(2);
        let mut n = 2;
        if content.is_some() {
            sets.push(format!("content = ${n}"));
            n += 1;
        }
        if importance.is_some() {
            sets.push(format!("importance = ${n}"));
        }
        let sql = format!("UPDATE memory SET {} WHERE id = $1", sets.join(", "));
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(id);
        if let Some(c) = content {
            q = q.bind(c);
        }
        if let Some(imp) = importance {
            q = q.bind(imp);
        }
        q.execute(&self.pool).await?;
        Ok(())
    }

    pub async fn by_ids(&self, ids: &[Uuid]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, Memory>("SELECT * FROM memory WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_by_id(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM memory WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
