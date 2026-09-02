use std::sync::Arc;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{model::Perception, repo::Repos, vo::Source},
    error::Result,
    util::pgvector,
};

#[derive(Debug)]
pub struct PerceptionService {
    repos: Repos,
    embedding: Arc<dyn EmbeddingService>,
}

impl PerceptionService {
    pub fn new(repos: Repos, embedding: Arc<dyn EmbeddingService>) -> Self {
        Self { repos, embedding }
    }

    pub async fn find(
        &self,
        source: &Source,
        parser: &str,
        focus: Option<&str>,
    ) -> Result<Option<Perception>> {
        self.repos.perception.find(source, parser, focus).await
    }

    /// 批量按 file_id 查询（单次 round-trip；每文件全行——基础转写 + 针对性判断）。
    pub async fn find_by_platform_file_ids(
        &self,
        file_ids: &[String],
    ) -> Result<Vec<(String, Perception)>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sources: Vec<serde_json::Value> = file_ids
            .iter()
            .map(|fid| serde_json::to_value(Source::platform("telegram", fid)))
            .collect::<serde_json::Result<_>>()?;
        let rows = self.repos.perception.find_by_sources(&sources).await?;
        Ok(rows
            .into_iter()
            .filter_map(|p| {
                let Ok(Source::Platform { file_id, .. }) =
                    serde_json::from_value::<Source>(p.source.clone())
                else {
                    return None;
                };
                Some((file_id, p))
            })
            .collect())
    }

    /// 批量按 URL 查询（单次 round-trip；全行）。
    pub async fn find_by_urls(&self, urls: &[String]) -> Result<Vec<Perception>> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }
        let sources: Vec<serde_json::Value> = urls
            .iter()
            .map(|u| serde_json::to_value(Source::url(u)))
            .collect::<serde_json::Result<_>>()?;
        self.repos.perception.find_by_sources(&sources).await
    }

    pub async fn upsert(
        &self,
        source: &Source,
        parser: &str,
        focus: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        if let Some(existing) = self.repos.perception.find(source, parser, focus).await? {
            self.repos
                .perception
                .update_content(existing.id, content)
                .await?;
            let id = existing.id;
            let content = content.to_string();
            let embedding = Arc::clone(&self.embedding);
            let pool = self.repos.pool().clone();
            tokio::spawn(async move {
                if let Err(e) =
                    pgvector::store_embedding(&*embedding, &pool, "perception", id, &content).await
                {
                    tracing::warn!(%id, "Failed to store perception embedding: {e}");
                }
            });
            return Ok(existing);
        }

        let perception = self
            .repos
            .perception
            .create(source, parser, focus, content)
            .await?;
        let id = perception.id;
        let content = content.to_string();
        let embedding = Arc::clone(&self.embedding);
        let pool = self.repos.pool().clone();
        tokio::spawn(async move {
            if let Err(e) =
                pgvector::store_embedding(&*embedding, &pool, "perception", id, &content).await
            {
                tracing::warn!(%id, "Failed to store perception embedding: {e}");
            }
        });

        Ok(perception)
    }
}
