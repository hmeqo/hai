use sqlx::PgPool;

use crate::{
    domain::{
        model::{Memory, MemoryKind, MemoryStatus, NewMemory},
        repo::{lexical, vector, window},
        vo::{AccountId, ConversationId, MemoryId, MessageId, TimeRange},
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct MemoryRepo {
    pool: PgPool,
}

impl MemoryRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_duplicate(
        &self,
        kind: MemoryKind,
        conversation_id: ConversationId,
        content: &str,
        account_id: Option<AccountId>,
    ) -> Result<Option<Memory>> {
        let mut sql = String::from(
            "SELECT * FROM memory WHERE kind = $1 AND conversation_id = $2 AND content = $3 \
             AND hashtextextended(content, 0) = hashtextextended($3, 0)",
        );
        if account_id.is_some() {
            sql.push_str(" AND account_id = $4");
        }
        sql.push_str(" LIMIT 1");
        let mut q = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(kind.as_str())
            .bind(conversation_id.0)
            .bind(content);
        if let Some(aid) = account_id {
            q = q.bind(aid.0);
        }
        q.fetch_optional(&self.pool).await.map_err(Into::into)
    }

    /// Writes the row and the vector it is found by in one statement.
    pub async fn create(&self, new: NewMemory, embedding: &[f32]) -> Result<Memory> {
        sqlx::query_as::<_, Memory>(
            "INSERT INTO memory (id, account_id, conversation_id, kind, content, source_message_ids, \
             status, created_at, updated_at, embedding) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::vector) RETURNING *",
        )
        .bind(new.id.0)
        .bind(new.account_id.map(|a| a.0))
        .bind(new.conversation_id.0)
        .bind(new.kind.as_str())
        .bind(new.content)
        .bind(new.source_message_ids)
        .bind(new.status.as_str())
        .bind(jiff_sqlx::Timestamp::from(new.created_at))
        .bind(jiff_sqlx::Timestamp::from(new.updated_at))
        .bind(vector::vec_to_pgstring(embedding))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(
        &self,
        conversation_id: ConversationId,
        id: MemoryId,
    ) -> Result<Option<Memory>> {
        sqlx::query_as::<_, Memory>("SELECT * FROM memory WHERE id = $1 AND conversation_id = $2")
            .bind(id.0)
            .bind(conversation_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn by_ids(&self, ids: &[MemoryId]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, Memory>("SELECT * FROM memory WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// Rewrites the content and the vector together, inside the chat's own rows.
    pub async fn revise(
        &self,
        conversation_id: ConversationId,
        id: MemoryId,
        content: &str,
        embedding: &[f32],
    ) -> Result<Option<Memory>> {
        sqlx::query_as::<_, Memory>(
            "UPDATE memory SET content = $3, embedding = $4::vector, updated_at = now() \
             WHERE id = $1 AND conversation_id = $2 RETURNING *",
        )
        .bind(id.0)
        .bind(conversation_id.0)
        .bind(content)
        .bind(vector::vec_to_pgstring(embedding))
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn set_status(
        &self,
        conversation_id: ConversationId,
        id: MemoryId,
        status: MemoryStatus,
    ) -> Result<Option<Memory>> {
        sqlx::query_as::<_, Memory>(
            "UPDATE memory SET status = $3, updated_at = now() \
             WHERE id = $1 AND conversation_id = $2 RETURNING *",
        )
        .bind(id.0)
        .bind(conversation_id.0)
        .bind(status.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn set_pinned(
        &self,
        conversation_id: ConversationId,
        id: MemoryId,
        pinned: bool,
    ) -> Result<Option<Memory>> {
        sqlx::query_as::<_, Memory>(
            "UPDATE memory SET pinned = $3, updated_at = now() \
             WHERE id = $1 AND conversation_id = $2 RETURNING *",
        )
        .bind(id.0)
        .bind(conversation_id.0)
        .bind(pinned)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn pinned(&self, conversation_id: ConversationId, limit: i64) -> Result<Vec<Memory>> {
        sqlx::query_as::<_, Memory>(
            "SELECT * FROM memory WHERE conversation_id = $1 AND pinned AND status = $2 \
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(conversation_id.0)
        .bind(MemoryStatus::Active.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Nearest memories by meaning; a memory that is no longer active is invisible to retrieval.
    pub async fn search_by_embedding(
        &self,
        conversation_id: ConversationId,
        query: &[f32],
        limit: i64,
    ) -> Result<Vec<(MemoryId, f64)>> {
        let filter = vector::Filter {
            column: "status",
            value: MemoryStatus::Active.as_str(),
        };
        let rows = vector::search_conversation_embedding_vec(
            &self.pool,
            vector::ConversationTable::Memory,
            query,
            conversation_id,
            Some(filter),
            limit,
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, distance)| (MemoryId(id), distance))
            .collect())
    }

    pub async fn search_by_keywords(
        &self,
        conversation_id: ConversationId,
        keywords: &[String],
        range: TimeRange,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let patterns = lexical::patterns(keywords);
        let query = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(keyword_search_sql().as_str()))
            .bind(conversation_id.0)
            .bind(MemoryStatus::Active.as_str());
        range
            .bind(query)
            .bind(&patterns)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn by_conversation_time(
        &self,
        conversation_id: ConversationId,
        range: TimeRange,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let sql = format!(
            "SELECT * FROM memory WHERE conversation_id = $1 AND status = $2 {} \
             ORDER BY created_at DESC LIMIT $5",
            window::time_window("created_at", 3)
        );
        let query = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(conversation_id.0)
            .bind(MemoryStatus::Active.as_str());
        range
            .bind(query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// Removes the row only when it belongs to this chat; `false` means there was nothing to remove,
    /// which the caller reports as a missing memory rather than a silent success.
    pub async fn delete(&self, conversation_id: ConversationId, id: MemoryId) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM memory WHERE id = $1 AND conversation_id = $2")
            .bind(id.0)
            .bind(conversation_id.0)
            .execute(&self.pool)
            .await?;
        Ok(deleted.rows_affected() > 0)
    }

    /// Deletes the memories a purge reaches: the ones recorded from a deleted message, and the ones
    /// created inside the window. An empty `message_ids` overlaps nothing, which is what a window
    /// without messages leaves.
    pub(crate) async fn delete_in_range_in_tx(
        tx: &mut sqlx::PgConnection,
        conversation_id: ConversationId,
        message_ids: &[MessageId],
        range: TimeRange,
    ) -> Result<u64> {
        let sql = format!(
            "DELETE FROM memory WHERE conversation_id = $1 \
             AND (source_message_ids && $2::bigint[] OR {})",
            window::predicate("created_at", 3),
        );
        let query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(conversation_id.0)
            .bind(message_ids);
        let deleted = range.bind_execute(query).execute(&mut *tx).await?;
        Ok(deleted.rows_affected())
    }
}

/// The lexical leg's statement: the keyword patterns are bound at `$5`.
fn keyword_search_sql() -> String {
    format!(
        "SELECT * FROM memory WHERE conversation_id = $1 AND status = $2 {} \
         AND {} \
         ORDER BY {}, \
                  created_at DESC LIMIT $6",
        window::time_window("created_at", 3),
        lexical::match_predicate(&["content"], 5),
        lexical::match_rank(&["content"], 5),
    )
}
