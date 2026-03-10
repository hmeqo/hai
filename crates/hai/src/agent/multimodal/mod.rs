pub mod audio;
pub mod embedding;
pub mod image;

use std::sync::Arc;

pub use audio::*;
pub use embedding::*;
pub use image::*;

use crate::agent::openrouter::RawOpenRouterClient;

pub struct MultimodalService {
    pub audio: Arc<AudioService>,
    pub image: Arc<ImageService>,
    pub embedding: Arc<EmbeddingService>,
}

impl MultimodalService {
    pub fn new(client: &RawOpenRouterClient) -> Self {
        Self {
            audio: Arc::new(AudioService::new(client)),
            image: Arc::new(ImageService::new(client)),
            embedding: Arc::new(EmbeddingService::new(client)),
        }
    }
}
