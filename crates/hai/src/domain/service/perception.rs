use crate::{
    agent::node::MultimodalService,
    domain::{model::Perception, vo::Source},
    error::Result,
};

#[derive(Debug)]
pub struct PerceptionService {
    db: toasty::Db,
    embedding: MultimodalService,
}

impl PerceptionService {
    pub fn new(db: toasty::Db, embedding: MultimodalService) -> Self {
        Self { db, embedding }
    }

    pub async fn find(
        &self,
        source: &Source,
        parser: &str,
        prompt: Option<&str>,
    ) -> Result<Option<Perception>> {
        let source_json = toasty::Json(serde_json::to_value(source)?);
        let prompt_eq = prompt.map(|s| Some(s.to_string()));
        Perception::filter(
            Perception::fields()
                .source()
                .eq(source_json)
                .and(Perception::fields().parser().eq(parser))
                .and(if let Some(ref p) = prompt_eq {
                    Perception::fields().prompt().eq(p.clone())
                } else {
                    Perception::fields().prompt().is_none()
                }),
        )
        .first()
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_platform_file_ids(
        &self,
        file_ids: &[String],
    ) -> Result<Vec<(String, Perception)>> {
        let mut results = Vec::new();
        for fid in file_ids {
            let source = Source::platform("telegram", fid);
            let source_json = toasty::Json(serde_json::to_value(&source)?);
            if let Some(p) = Perception::filter(Perception::fields().source().eq(source_json))
                .first()
                .exec(&mut self.db.clone())
                .await?
            {
                results.push((fid.clone(), p));
            }
        }
        Ok(results)
    }

    pub async fn find_by_urls(&self, urls: &[String]) -> Result<Vec<Perception>> {
        let mut results = Vec::new();
        for url in urls {
            let source = Source::url(url);
            let source_json = toasty::Json(serde_json::to_value(&source)?);
            if let Some(p) = Perception::filter(Perception::fields().source().eq(source_json))
                .first()
                .exec(&mut self.db.clone())
                .await?
            {
                results.push(p);
            }
        }
        Ok(results)
    }

    pub async fn upsert(
        &self,
        source: &Source,
        parser: &str,
        prompt: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        let source_json = toasty::Json(serde_json::to_value(source)?);
        let mut db = self.db.clone();
        let prompt_opt = prompt.map(|s| s.to_string());

        if let Some(mut existing) = Perception::filter(
            Perception::fields()
                .source()
                .eq(source_json.clone())
                .and(Perception::fields().parser().eq(parser))
                .and(if let Some(ref p) = prompt_opt {
                    Perception::fields().prompt().eq(Some(p.clone()))
                } else {
                    Perception::fields().prompt().is_none()
                }),
        )
        .first()
        .exec(&mut db)
        .await?
        {
            toasty::update!(existing { content }).exec(&mut db).await?;
            if let Ok(embedding) = self.embedding.generate_embedding(content).await {
                toasty::update!(existing {
                    embedding: Some(toasty::Json(embedding))
                })
                .exec(&mut db)
                .await
                .ok();
            }
            return Ok(existing);
        }

        let mut perception = toasty::create!(Perception {
            id: uuid::Uuid::now_v7(),
            source: source_json,
            parser,
            prompt: prompt_opt,
            content,
            created_at: jiff::Timestamp::now(),
        })
        .exec(&mut db)
        .await?;

        if let Ok(embedding) = self.embedding.generate_embedding(content).await {
            toasty::update!(perception {
                embedding: Some(toasty::Json(embedding))
            })
            .exec(&mut db)
            .await
            .ok();
        }

        Ok(perception)
    }
}
