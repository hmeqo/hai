use anyhow::Result;
use reqwest::Client;
use serde_json::{Value, json};
use std::sync::Arc;

/// OpenAI 兼容的 HTTP 客户端
///
/// 用于多模态服务（embedding、图像生成、音频分析）的底层 HTTP 通信。
/// 支持任何 OpenAI 兼容 API（OpenAI, OpenRouter, DeepSeek, Groq 等）。
#[derive(Debug)]
pub struct RawClientInner {
    base_url: String,
    api_key: String,
    client: Client,
}

impl RawClientInner {
    fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawClient {
    inner: Arc<RawClientInner>,
}

impl RawClient {
    /// 使用指定的 base_url 创建客户端
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RawClientInner::new(api_key, base_url)),
        }
    }

    /// 使用 OpenRouter 默认地址创建客户端（向后兼容）
    pub fn openrouter(api_key: impl Into<String>) -> Self {
        Self::new(api_key, "https://openrouter.ai/api/v1")
    }

    /// 使用 OpenAI 默认地址创建客户端
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::new(api_key, "https://api.openai.com/v1")
    }

    pub fn model(&self, model: impl Into<String>) -> RawModel {
        RawModel::new(self.clone(), model)
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
pub struct RawModel {
    client: Arc<RawClient>,
    model: String,
}

impl RawModel {
    pub fn new(client: RawClient, model: impl Into<String>) -> Self {
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
