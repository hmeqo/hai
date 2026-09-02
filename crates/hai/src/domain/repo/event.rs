use serde_json::Value;
use sqlx::PgPool;

use crate::{domain::model::Event, error::Result};

#[derive(Debug, Clone)]
pub struct EventRepo {
    pool: PgPool,
}

impl EventRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, domain: &str, payload: Value) -> Result<Event> {
        sqlx::query_as::<_, Event>(
            "INSERT INTO event (domain, payload) VALUES ($1, $2) RETURNING *",
        )
        .bind(domain)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// 事件查询：可选 JSONB 过滤（chat_id / kind tag）、seq 范围、方向与 limit。
    /// 结果按 seq 排序（`desc=true` 降序 / `false` 升序）。
    pub async fn query(
        &self,
        chat_id: Option<i64>,
        kind: Option<&str>,
        before_seq: Option<i64>,
        after_seq: Option<i64>,
        desc: bool,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let mut sql =
            String::from("SELECT seq, domain, payload, created_at FROM event WHERE seq > 0");
        let mut n = 0usize;
        if let Some(_cid) = chat_id {
            n += 1;
            sql.push_str(&format!(" AND (payload->>'chat_id')::bigint = ${n}"));
        }
        if let Some(_k) = kind {
            n += 1;
            sql.push_str(&format!(" AND payload->'payload'->>'event' = ${n}"));
        }
        if let Some(_b) = before_seq {
            n += 1;
            sql.push_str(&format!(" AND seq < ${n}"));
        }
        if let Some(_a) = after_seq {
            n += 1;
            sql.push_str(&format!(" AND seq > ${n}"));
        }
        let limit_idx = n + 1;
        sql.push_str(&format!(
            " ORDER BY seq {} LIMIT ${limit_idx}",
            if desc { "DESC" } else { "ASC" },
        ));

        let mut q = sqlx::query_as::<_, Event>(sqlx::AssertSqlSafe(sql.as_str()));
        if let Some(cid) = chat_id {
            q = q.bind(cid);
        }
        if let Some(k) = kind {
            q = q.bind(k);
        }
        if let Some(b) = before_seq {
            q = q.bind(b);
        }
        if let Some(a) = after_seq {
            q = q.bind(a);
        }
        // LIMIT 占位符编号最大（$limit_idx），最后绑定
        q = q.bind(limit as i64);
        Ok(q.fetch_all(&self.pool).await?)
    }

    pub async fn by_seq(&self, seq: i64) -> Result<Option<Event>> {
        sqlx::query_as::<_, Event>(
            "SELECT seq, domain, payload, created_at \
             FROM event WHERE seq = $1 LIMIT 1",
        )
        .bind(seq)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// 统计 seq > after 的匹配事件数（chat_id/kind 可选过滤）。
    pub async fn count_after(
        &self,
        after_seq: i64,
        chat_id: Option<i64>,
        kind: Option<&str>,
    ) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM event WHERE seq > $1");
        let mut n = 1usize;
        if let Some(_cid) = chat_id {
            n += 1;
            sql.push_str(&format!(" AND (payload->>'chat_id')::bigint = ${n}"));
        }
        if let Some(_k) = kind {
            n += 1;
            sql.push_str(&format!(" AND payload->'payload'->>'event' = ${n}"));
        }
        let mut q = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql.as_str())).bind(after_seq);
        if let Some(cid) = chat_id {
            q = q.bind(cid);
        }
        if let Some(k) = kind {
            q = q.bind(k);
        }
        Ok(q.fetch_one(&self.pool).await?)
    }
}
