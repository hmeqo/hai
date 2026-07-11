use std::io::Cursor;

use serde_json::Value;

use crate::{
    error::{ErrorKind, Result},
    util::url::join_url,
};

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// 统一 HTTP 客户端。无状态，连接参数由 `Endpoint` 传入。
#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .pool_max_idle_per_host(10)
                .pool_idle_timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build reqwest client"),
        }
    }

    /// POST JSON 并返回 JSON 响应。非 2xx 自动转 Err。
    async fn request_json(&self, ep: &Endpoint, path: &str, body: Value) -> Result<Value> {
        let resp = self
            .http
            .post(join_url(&[&ep.base_url, path]))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", ep.api_key))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(
                ErrorKind::Internal.msg(format!("API request failed (HTTP {status}): {text}"))
            );
        }
        Ok(resp.json().await?)
    }

    /// POST JSON 并返回原始字节 + content-type。
    async fn request_bytes(
        &self,
        ep: &Endpoint,
        path: &str,
        body: Value,
    ) -> Result<(Vec<u8>, String)> {
        let resp = self
            .http
            .post(join_url(&[&ep.base_url, path]))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", ep.api_key))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(
                ErrorKind::Internal.msg(format!("API request failed (HTTP {status}): {text}"))
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

    /// 聊天补全。POST {base_url}/chat/completions
    pub async fn complete(&self, ep: &Endpoint, content: Value) -> Result<Value> {
        let body = serde_json::json!({
            "model": &ep.model,
            "messages": [{"role": "user", "content": content}],
        });
        self.request_json(ep, "chat/completions", body).await
    }

    /// 文本嵌入。POST {base_url}/embeddings
    pub async fn embed(&self, ep: &Endpoint, input: &str) -> Result<Vec<f32>> {
        let body = serde_json::json!({ "model": &ep.model, "input": input });
        let resp: Value = self.request_json(ep, "embeddings", body).await?;
        let arr = resp["data"][0]["embedding"].as_array().ok_or_else(|| {
            ErrorKind::DataParse.msg(format!("Failed to get embedding: {resp:?}"))
        })?;
        Ok(arr.iter().map(|v| v.as_f64().unwrap() as f32).collect())
    }

    /// 语音合成。POST {base_url}/audio/speech，PCM 自动转 WAV。
    pub async fn speech(
        &self,
        ep: &Endpoint,
        input: &str,
        voice: &str,
        speed: f32,
    ) -> Result<Vec<u8>> {
        let body = serde_json::json!({
            "model": &ep.model,
            "input": input,
            "voice": voice,
            "speed": speed.clamp(0.25, 4.0),
        });
        let (bytes, content_type) = self.request_bytes(ep, "audio/speech", body).await?;
        if content_type.contains("pcm") || content_type.contains("L8") {
            Ok(pcm_to_wav(&bytes))
        } else {
            Ok(bytes)
        }
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate = 24000u32;
    let bits_per_sample = 16;
    let channels = 1;
    let samples: Vec<i16> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
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
