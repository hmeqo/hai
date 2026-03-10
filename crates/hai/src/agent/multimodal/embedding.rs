use anyhow::Result;

use crate::agent::openrouter::{RawOpenRouterClient, RawOpenRouterModel};

#[derive(Clone)]
pub struct EmbeddingService {
    model: RawOpenRouterModel,
}

impl EmbeddingService {
    pub fn new(client: &RawOpenRouterClient) -> Self {
        Self {
            model: client.model("openai/text-embedding-3-small"),
        }
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.model.embedding(text).await
    }
}
