use sqlx::PgPool;

use crate::{
    domain::{model::Perception, vo::Source},
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

    /// 按 (source, parser, focus) 精确查重；focus None 匹配 `prompt IS NULL`。
    pub async fn find(
        &self,
        source: &Source,
        parser: &str,
        focus: Option<&str>,
    ) -> Result<Option<Perception>> {
        let source_json = serde_json::to_value(source)?;
        sqlx::query_as::<_, Perception>(
            "SELECT * FROM perception \
             WHERE source = $1 AND parser = $2 \
               AND (prompt = $3 OR (prompt IS NULL AND $3 IS NULL)) \
             LIMIT 1",
        )
        .bind(source_json)
        .bind(parser)
        .bind(focus)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// 按 source 数组批量查询：每 source 返回全部行（基础转写 + 针对性判断双层都取——
    /// `find_by_source` 的 LIMIT 1 会吞掉一层，渲染双层合并依赖全行）。
    pub async fn find_by_sources(&self, sources: &[serde_json::Value]) -> Result<Vec<Perception>> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=sources.len()).map(|i| format!("${i}")).collect();
        let sql = format!(
            "SELECT * FROM perception WHERE source IN ({})",
            placeholders.join(", ")
        );
        let mut q = sqlx::query_as::<_, Perception>(sqlx::AssertSqlSafe(sql.as_str()));
        for s in sources {
            q = q.bind(s);
        }
        q.fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn update_content(&self, id: uuid::Uuid, content: &str) -> Result<()> {
        sqlx::query("UPDATE perception SET content = $2 WHERE id = $1")
            .bind(id)
            .bind(content)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create(
        &self,
        source: &Source,
        parser: &str,
        focus: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        let source_json = serde_json::to_value(source)?;
        sqlx::query_as::<_, Perception>(
            "INSERT INTO perception (id, source, parser, prompt, content) \
             VALUES ($1, $2, $3, $4, $5) RETURNING *",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(source_json)
        .bind(parser)
        .bind(focus)
        .bind(content)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }
}
