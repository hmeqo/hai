use anyhow::Result;

use crate::agent::rawclient::{RawClient, RawModel};

#[derive(Clone)]
pub struct EmbeddingService {
    model: RawModel,
}

impl EmbeddingService {
    pub fn new(client: &RawClient, model: &str) -> Self {
        Self {
            model: client.model(model),
        }
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.model.embedding(text).await
    }
}
