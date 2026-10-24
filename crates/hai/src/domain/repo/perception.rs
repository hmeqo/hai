use sqlx::PgPool;

use crate::{
    domain::{
        model::Perception,
        repo::{vector, window},
        vo::{AttachmentParser, ConversationId, PerceptionId, Subject, SubjectKind, TimeRange},
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct PerceptionRepo {
    pool: PgPool,
}

impl PerceptionRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Exact cache lookup on (subject_kind, subject, parser, prompt); `None` matches the
    /// `prompt IS NULL` row.
    pub async fn find(
        &self,
        conversation_id: ConversationId,
        subject: &Subject,
        parser: AttachmentParser,
        focus: Option<&str>,
    ) -> Result<Option<Perception>> {
        sqlx::query_as::<_, Perception>(
            "SELECT * FROM perception \
             WHERE conversation_id = $1 AND subject_kind = $2 AND subject = $3 \
               AND parser = $4 AND (prompt = $5 OR (prompt IS NULL AND $5 IS NULL)) \
             LIMIT 1",
        )
        .bind(conversation_id.0)
        .bind(subject.kind.name())
        .bind(&subject.value)
        .bind(parser.name())
        .bind(focus)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// One round trip for many subjects of the same kind.
    pub async fn find_by_subjects(
        &self,
        conversation_id: ConversationId,
        kind: SubjectKind,
        subjects: &[String],
    ) -> Result<Vec<Perception>> {
        if subjects.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, Perception>(
            "SELECT * FROM perception \
             WHERE conversation_id = $1 AND subject_kind = $2 AND subject = ANY($3)",
        )
        .bind(conversation_id.0)
        .bind(kind.name())
        .bind(subjects)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Writes the vector the row is found by; the caller generates it and decides what a failure
    /// means.
    pub async fn store_embedding(&self, id: PerceptionId, embedding: &[f32]) -> Result<()> {
        vector::upsert_embedding_vec(
            &self.pool,
            vector::EmbeddingTable::Perception,
            id.0,
            embedding,
        )
        .await
    }

    /// Returns the row as it is after the write: a caller handed the row it read would be given the
    /// content this call replaced.
    pub async fn update_content(
        &self,
        id: PerceptionId,
        content: &str,
    ) -> Result<Option<Perception>> {
        sqlx::query_as::<_, Perception>(
            "UPDATE perception SET content = $2 WHERE id = $1 RETURNING *",
        )
        .bind(id.0)
        .bind(content)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn create(
        &self,
        conversation_id: ConversationId,
        subject: &Subject,
        parser: AttachmentParser,
        focus: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        sqlx::query_as::<_, Perception>(
            "INSERT INTO perception (id, conversation_id, subject_kind, subject, parser, prompt, content) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(conversation_id.0)
        .bind(subject.kind.name())
        .bind(&subject.value)
        .bind(parser.name())
        .bind(focus)
        .bind(content)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// The analyses this Conversation's purge reaches: every subject, every parser and focus, written
    /// inside the window. A perception carries its Conversation, so a URL cache row is in scope too.
    pub async fn delete_in_range_for_conversation(
        &self,
        conversation_id: ConversationId,
        range: TimeRange,
    ) -> Result<u64> {
        let sql = format!(
            "DELETE FROM perception WHERE conversation_id = $1 {}",
            window::time_window("created_at", 2)
        );
        let query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(conversation_id.0);
        let deleted = range.bind_execute(query).execute(&self.pool).await?;
        Ok(deleted.rows_affected())
    }
}
