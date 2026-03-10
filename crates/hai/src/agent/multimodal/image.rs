use anyhow::Result;
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Value, json};

use crate::agent::openrouter::{RawOpenRouterClient, RawOpenRouterModel};

#[derive(Debug, Clone)]
pub struct ImageService {
    model: RawOpenRouterModel,
}

impl ImageService {
    pub fn new(client: &RawOpenRouterClient) -> Self {
        Self {
            model: client.model("google/gemini-3.1-flash-image-preview"),
        }
    }

    pub async fn generate_image(&self, prompt: &str) -> Result<DataImage> {
        let content = json!([
            {"type": "text", "text": prompt}
        ]);

        self.generate_image_with_content(content).await
    }

    pub async fn generate_image_with_image_url(
        &self,
        prompt: &str,
        image_url: &str,
    ) -> Result<DataImage> {
        let content = json!([
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": image_url}}
        ]);

        self.generate_image_with_content(content).await
    }

    pub async fn generate_image_with_content(&self, content: Value) -> Result<DataImage> {
        let resp = self
            .model
            .generate(
                content,
                Some(json!({
                    "modalities": ["image", "text"],
                    // "image_config": {
                    //     "aspect_ratio": "16:9",
                    //     "image_size": "4K"
                    // }
                })),
            )
            .await?;

        if let Some(images) = resp
            .get("choices")
            .and_then(|x| x.get(0))
            .and_then(|x| x.get("message"))
            .and_then(|x| x.get("images"))
        {
            for image in images
                .as_array()
                .ok_or(anyhow::anyhow!("No images found"))?
            {
                if let Some(image_url) = image.get("image_url").and_then(|x| x.get("url")) {
                    return Ok(DataImage(
                        image_url
                            .as_str()
                            .ok_or(anyhow::anyhow!("No image url"))?
                            .into(),
                    ));
                }
            }
        }

        Err(anyhow::anyhow!("No images found"))
    }
}

#[derive(Debug, Clone)]
pub struct DataImage(String);

impl DataImage {
    pub async fn base64_url(&self) -> &str {
        &self.0
    }

    pub async fn tmp_file(&self) -> Result<TmpFileGuard> {
        let image_data = base64url_to_data(&self.0)?;

        let path = format!(
            "/tmp/image_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
        );
        tokio::fs::write(&path, image_data).await?;
        Ok(TmpFileGuard { path })
    }
}

#[derive(Debug, Clone)]
pub struct TmpFileGuard {
    path: String,
}

impl TmpFileGuard {
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).unwrap();
    }
}

pub fn base64url_to_data(base64url: &str) -> Result<Vec<u8>> {
    let base64_data = base64url
        .split(',')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Invalid data URL"))?;
    BASE64_STANDARD.decode(base64_data).map_err(Into::into)
}
