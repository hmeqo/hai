pub mod audio;
pub mod embedding;
pub mod image;

use std::sync::Arc;

pub use audio::*;
pub use embedding::*;
pub use image::*;

use crate::agent::rawclient::RawClient;
use crate::config::provider_manager::ProviderManager;
use crate::config::schema::MultimodalConfig;
use crate::error::Result;

pub struct MultimodalService {
    pub audio: Arc<AudioService>,
    pub image: Arc<ImageService>,
    pub embedding: Arc<EmbeddingService>,
}

impl MultimodalService {
    pub fn new(
        providers: &ProviderManager,
        config: &MultimodalConfig,
        default_provider: &str,
    ) -> Result<Self> {
        let image_provider = Self::resolve_provider(&config.image.provider, default_provider);
        let audio_provider = Self::resolve_provider(&config.audio.provider, default_provider);
        let embedding_provider =
            Self::resolve_provider(&config.embedding.provider, default_provider);

        let image_client = Self::build_client(providers.get_checked(&image_provider)?);
        let audio_client = Self::build_client(providers.get_checked(&audio_provider)?);
        let embedding_client = Self::build_client(providers.get_checked(&embedding_provider)?);

        Ok(Self {
            audio: Arc::new(AudioService::new(&audio_client, &config.audio.model)),
            image: Arc::new(ImageService::new(&image_client, &config.image.model)),
            embedding: Arc::new(EmbeddingService::new(
                &embedding_client,
                &config.embedding.model,
            )),
        })
    }

    fn resolve_provider(provider: &str, default: &str) -> String {
        if provider.is_empty() {
            default.to_string()
        } else {
            provider.to_string()
        }
    }

    fn build_client(resolved: &crate::config::schema::ResolvedProvider) -> RawClient {
        RawClient::new(&resolved.config.api_key, &resolved.base_url)
    }
}
