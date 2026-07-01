use crate::{agentcore::rawclient::RawAgent, config::schema::TtsConfig, error::Result};

#[derive(Debug)]
pub struct TtsService {
    speech: RawAgent,
    voice: String,
    speed: f32,
}

impl TtsService {
    pub fn new(speech_agent: RawAgent, config: &TtsConfig) -> Self {
        Self {
            speech: speech_agent,
            voice: config.voice.clone(),
            speed: config.speed,
        }
    }

    pub async fn speech(&self, prompt: &str) -> Result<Vec<u8>> {
        self.speech.speech(prompt, &self.voice, self.speed).await
    }
}
