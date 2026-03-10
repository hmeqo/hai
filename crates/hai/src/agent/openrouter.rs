use anyhow::Result;
use reqwest::Client;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug)]
pub struct RawOpenRouterClientInner {
    base_url: String,
    api_key: String,
    client: Client,
}

impl RawOpenRouterClientInner {
    fn new(api_key: impl Into<String>) -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(10) // 最多保持 10 个空闲连接
            .pool_idle_timeout(std::time::Duration::from_secs(30)) // 空闲连接 30 秒后关闭
            .build()
            .expect("Failed to build reqwest client");

        Self {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: api_key.into(),
            client,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawOpenRouterClient {
    inner: Arc<RawOpenRouterClientInner>,
}

impl RawOpenRouterClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RawOpenRouterClientInner::new(api_key)),
        }
    }

    pub fn model(&self, model: impl Into<String>) -> RawOpenRouterModel {
        RawOpenRouterModel::new(self.clone(), model)
    }

    async fn request(&self, sub_url: &str, body: &Value) -> Result<Value> {
        Ok(self
            .inner
            .client
            .post(format!("{}{}", self.inner.base_url, sub_url))
            .header("Authorization", format!("Bearer {}", &self.inner.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?
            .json::<Value>()
            .await?)
    }
}

#[derive(Debug, Clone)]
pub struct RawOpenRouterModel {
    client: Arc<RawOpenRouterClient>,
    model: String,
}

impl RawOpenRouterModel {
    pub fn new(client: RawOpenRouterClient, model: impl Into<String>) -> Self {
        Self {
            client: Arc::new(client),
            model: model.into(),
        }
    }

    pub async fn generate(
        &self,
        content: impl Into<Value>,
        extra_body: Option<impl Into<Value>>,
    ) -> Result<Value> {
        let mut body = json!({
            "model": &self.model,
            "messages": [
                {"role": "user", "content": content.into()}
            ],
        });

        if let Some(extra_body) = extra_body
            && let (Value::Object(map_a), Value::Object(map_b)) = (&mut body, extra_body.into())
        {
            map_a.extend(map_b);
        }

        let response = self.client.request("/chat/completions", &body).await?;

        Ok(response)
    }

    pub async fn embedding(&self, input: &str) -> Result<Vec<f32>> {
        let body = json!({
            "model": &self.model,
            "input": input,
        });

        let resp = self.client.request("/embeddings", &body).await?;

        let embedding = resp["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Failed to get embedding: {:?}", resp))?
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();

        Ok(embedding)
    }
}
