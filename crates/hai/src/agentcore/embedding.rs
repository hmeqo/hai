use std::fmt::Debug;

use async_trait::async_trait;

use crate::error::Result;

#[async_trait]
pub trait EmbeddingService: Debug + Send + Sync {
    async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>>;
}
