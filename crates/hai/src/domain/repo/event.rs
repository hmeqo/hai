use serde_json::Value;
use sqlx::PgPool;

use crate::{
    domain::{
        model::Event,
        repo::window,
        vo::{ChatId, ConversationId, EventKind, TimeRange},
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct EventRepo {
    pool: PgPool,
}

/// One page of the log: the filters and the seq window it reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventQuery<'a> {
    pub chat: Option<ChatId>,
    pub kind: Option<EventKind>,
    /// A session id prefix; matched against the stored id, case-insensitively.
    pub session: Option<&'a str>,
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub desc: bool,
    pub limit: usize,
}

impl EventRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, payload: Value) -> Result<Event> {
        sqlx::query_as::<_, Event>("INSERT INTO event (payload) VALUES ($1) RETURNING *")
            .bind(payload)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn query(&self, page: EventQuery<'_>) -> Result<Vec<Event>> {
        let sql = query_sql(
            page.chat,
            page.kind,
            page.session,
            page.before,
            page.after,
            page.desc,
        );

        let mut stmt = sqlx::query_as::<_, Event>(sqlx::AssertSqlSafe(sql.as_str()));
        if let Some(cid) = page.chat {
            stmt = stmt.bind(cid.0);
        }
        if let Some(k) = page.kind {
            stmt = stmt.bind(k.as_str());
        }
        if let Some(prefix) = page.session {
            stmt = stmt.bind(format!("{}%", prefix.to_lowercase()));
        }
        if let Some(b) = page.before {
            stmt = stmt.bind(b);
        }
        if let Some(a) = page.after {
            stmt = stmt.bind(a);
        }
        stmt = stmt.bind(page.limit as i64);
        Ok(stmt.fetch_all(&self.pool).await?)
    }

    pub async fn by_seq(&self, seq: i64) -> Result<Option<Event>> {
        sqlx::query_as::<_, Event>(
            "SELECT seq, payload, created_at \
             FROM event WHERE seq = $1 LIMIT 1",
        )
        .bind(seq)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn count_after(
        &self,
        after_seq: i64,
        chat_id: Option<ChatId>,
        kind: Option<EventKind>,
        session: Option<&str>,
    ) -> Result<i64> {
        let sql = count_after_sql(chat_id, kind, session);
        let mut q = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql.as_str())).bind(after_seq);
        if let Some(cid) = chat_id {
            q = q.bind(cid.0);
        }
        if let Some(k) = kind {
            q = q.bind(k.as_str());
        }
        if let Some(prefix) = session {
            q = q.bind(format!("{}%", prefix.to_lowercase()));
        }
        Ok(q.fetch_one(&self.pool).await?)
    }

    pub async fn delete_in_range_for_conversation(
        &self,
        conversation_id: ConversationId,
        range: TimeRange,
    ) -> Result<u64> {
        let sql = format!(
            "DELETE FROM event WHERE (payload->>'chat_id')::bigint IN \
             (SELECT id FROM chat WHERE conversation_id = $1) {}",
            window::time_window("created_at", 2),
        );
        let query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(conversation_id.0);
        let deleted = range.bind_execute(query).execute(&self.pool).await?;
        Ok(deleted.rows_affected())
    }
}

/// Appends the `chat_id` and `event` predicates a filtered query carries, each with its leading
/// space, and returns the index of the last placeholder bound. The JSONB spellings must keep
/// matching the expression indexes in `schema.sql`.
fn push_filters(
    sql: &mut String,
    n: usize,
    chat_id: Option<ChatId>,
    kind: Option<EventKind>,
    session: Option<&str>,
) -> usize {
    let mut n = n;
    if chat_id.is_some() {
        n += 1;
        sql.push_str(&format!(" AND (payload->>'chat_id')::bigint = ${n}"));
    }
    if kind.is_some() {
        n += 1;
        sql.push_str(&format!(" AND payload->'payload'->>'event' = ${n}"));
    }
    if session.is_some() {
        n += 1;
        sql.push_str(&format!(" AND (payload->'session'->>'id') LIKE ${n}"));
    }
    n
}

/// The statement `query` runs: the shared predicate, then the sequence window, then `LIMIT` last.
fn query_sql(
    chat_id: Option<ChatId>,
    kind: Option<EventKind>,
    session: Option<&str>,
    before_seq: Option<i64>,
    after_seq: Option<i64>,
    desc: bool,
) -> String {
    let mut sql = String::from("SELECT seq, payload, created_at FROM event WHERE seq > 0");
    let mut n = push_filters(&mut sql, 0, chat_id, kind, session);
    if before_seq.is_some() {
        n += 1;
        sql.push_str(&format!(" AND seq < ${n}"));
    }
    if after_seq.is_some() {
        n += 1;
        sql.push_str(&format!(" AND seq > ${n}"));
    }
    let limit_idx = n + 1;
    sql.push_str(&format!(
        " ORDER BY seq {} LIMIT ${limit_idx}",
        if desc { "DESC" } else { "ASC" },
    ));
    sql
}

/// The statement `count_after` runs: `after_seq` owns `$1`, so the shared predicate starts at `$2`.
fn count_after_sql(
    chat_id: Option<ChatId>,
    kind: Option<EventKind>,
    session: Option<&str>,
) -> String {
    let mut sql = String::from("SELECT COUNT(*) FROM event WHERE seq > $1");
    push_filters(&mut sql, 1, chat_id, kind, session);
    sql
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statement_binds_its_filters_windows_and_limit_in_order() {
        let chat = Some(ChatId(7));
        assert_eq!(
            query_sql(None, None, None, None, None, false),
            "SELECT seq, payload, created_at FROM event WHERE seq > 0 \
             ORDER BY seq ASC LIMIT $1"
        );
        assert_eq!(
            query_sql(
                chat,
                Some(EventKind::ToolCall),
                None,
                Some(9),
                Some(3),
                true
            ),
            "SELECT seq, payload, created_at FROM event WHERE seq > 0 \
             AND (payload->>'chat_id')::bigint = $1 AND payload->'payload'->>'event' = $2 \
             AND seq < $3 AND seq > $4 ORDER BY seq DESC LIMIT $5"
        );
        assert_eq!(
            query_sql(None, None, Some("3f9c"), None, None, false),
            "SELECT seq, payload, created_at FROM event WHERE seq > 0 \
             AND (payload->'session'->>'id') LIKE $1 ORDER BY seq ASC LIMIT $2"
        );
        assert_eq!(
            count_after_sql(chat, Some(EventKind::ToolCall), None),
            "SELECT COUNT(*) FROM event WHERE seq > $1 \
             AND (payload->>'chat_id')::bigint = $2 AND payload->'payload'->>'event' = $3"
        );
        assert_eq!(
            count_after_sql(None, None, None),
            "SELECT COUNT(*) FROM event WHERE seq > $1"
        );
    }
}
