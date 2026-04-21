use anyhow::Result;
use serde_json::json;

use crate::agent::rawclient::{RawClient, RawModel};

#[derive(Debug, Clone)]
pub struct AudioService {
    model: RawModel,
}

impl AudioService {
    pub fn new(client: &RawClient, model: &str) -> Self {
        Self {
            model: client.model(model),
        }
    }

    pub async fn analyze_audio(
        &self,
        prompt: &str,
        input_audio: &str,
        format: &str,
    ) -> Result<String> {
        let resp = self
            .model
            .generate(
                json!([
                    {"type": "text", "text": prompt},
                    {"type": "input_audio", "input_audio": {
                        "data": input_audio,
                        "format": format
                    }}
                ]),
                None::<()>,
            )
            .await?;

        Ok(resp.to_string())
    }
}
