use std::{io::Cursor, sync::Arc};

use reqwest::Client;
use serde_json::{Value, json};

use crate::error::{AppResultExt, ErrorKind, OptionAppExt, Result};

#[derive(Debug)]
struct RawClientInner {
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
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RawClientInner::new(api_key, base_url)),
        }
    }

    pub fn openrouter(api_key: impl Into<String>) -> Self {
        Self::new(api_key, "https://openrouter.ai/api/v1")
    }

    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::new(api_key, "https://api.openai.com/v1")
    }

    pub fn agent(&self, model: impl Into<String>) -> RawAgent {
        RawAgent::new(self.clone(), model)
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

    pub async fn request_bytes(&self, sub_url: &str, body: &Value) -> Result<Vec<u8>> {
        let (bytes, _) = self.request_bytes_with_type(sub_url, body).await?;
        Ok(bytes)
    }

    pub async fn request_bytes_with_type(
        &self,
        sub_url: &str,
        body: &Value,
    ) -> Result<(Vec<u8>, String)> {
        let resp = self
            .inner
            .client
            .post(format!("{}{}", self.inner.base_url, sub_url))
            .header("Authorization", format!("Bearer {}", &self.inner.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(
                ErrorKind::Internal.with_msg(format!("request failed (HTTP {status}): {body}"))
            );
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = resp.bytes().await?.to_vec();
        Ok((bytes, content_type))
    }
}

#[derive(Debug, Clone)]
pub struct RawAgent {
    pub client: Arc<RawClient>,
    pub model: Arc<String>,
    pub default_prompt: Arc<String>,
}

impl RawAgent {
    pub fn new(client: RawClient, name: impl Into<String>) -> Self {
        Self {
            client: Arc::new(client),
            model: Arc::new(name.into()),
            default_prompt: Arc::new(String::new()),
        }
    }

    pub fn with_default_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.default_prompt = Arc::new(prompt.into());
        self
    }

    pub async fn completion(
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
            .ok_or_err_msg(
                ErrorKind::DataParse,
                format!("Failed to get embedding: {:?}", resp),
            )?
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();

        Ok(embedding)
    }

    pub async fn speech(&self, input: &str, voice: &str, speed: f32) -> Result<Vec<u8>> {
        let body = json!({
            "model": &self.model,
            "input": input,
            "voice": voice,
            "speed": speed.clamp(0.25, 4.0),
        });

        let (bytes, content_type) = self
            .client
            .request_bytes_with_type("/audio/speech", &body)
            .await
            .change_err_msg(ErrorKind::Internal, "TTS synthesis failed")?;

        // PCM → WAV（Telegram sendVoice 不接受裸 PCM）
        if content_type.contains("pcm") || content_type.contains("L8") {
            Ok(pcm_to_wav(&bytes))
        } else {
            Ok(bytes)
        }
    }
}

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate = 24000u32;
    let bits_per_sample = 16;
    let channels = 1;
    let samples: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buf = Cursor::new(Vec::with_capacity(44 + pcm.len()));
    let mut writer = hound::WavWriter::new(&mut buf, spec).unwrap();
    for s in samples {
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
    buf.into_inner()
}
