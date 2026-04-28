use pgvector::Vector;
use sqlx::PgPool;

use crate::{
    agentcore::multimodal::MultimodalService,
    domain::{entity::Perception, repo::PerceptionRepo, vo::Source},
    error::Result,
};

#[derive(Debug)]
pub struct PerceptionService {
    pool: PgPool,
    embedding: MultimodalService,
}

impl PerceptionService {
    pub fn new(pool: PgPool, embedding: MultimodalService) -> Self {
        Self { pool, embedding }
    }

    pub async fn find(
        &self,
        source: &Source,
        parser: &str,
        prompt: Option<&str>,
    ) -> Result<Option<Perception>> {
        let source_json = serde_json::to_value(source)?;
        PerceptionRepo::find(&self.pool, &source_json, parser, prompt).await
    }

    /// 批量按 file_id 查询平台附件的 perception 结果。
    /// 返回 `(file_id, Perception)` 列表。
    pub async fn find_by_platform_file_ids(
        &self,
        file_ids: &[String],
    ) -> Result<Vec<(String, Perception)>> {
        PerceptionRepo::find_by_platform_file_ids(&self.pool, file_ids).await
    }

    /// 批量查询一组 URL 对应的 perception 结果。
    pub async fn find_by_urls(&self, urls: &[String]) -> Result<Vec<Perception>> {
        PerceptionRepo::find_by_urls(&self.pool, urls).await
    }

    /// upsert perception 并异步写入 embedding（生成失败不影响主流程）
    pub async fn upsert(
        &self,
        source: &Source,
        parser: &str,
        prompt: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        let source_json = serde_json::to_value(source)?;
        let perception =
            PerceptionRepo::upsert(&self.pool, &source_json, parser, prompt, content).await?;

        if let Ok(vec) = self
            .embedding
            .generate_embedding(content)
            .await
            .map(Vector::from)
        {
            let _ = PerceptionRepo::set_embedding(&self.pool, perception.id, &vec).await;
        }

        Ok(perception)
    }
}
