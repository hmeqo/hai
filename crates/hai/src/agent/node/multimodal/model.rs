use std::collections::HashMap;

use crate::config::schema::{GenerationModelConfig, ModalityType};

pub struct ModelService {
    models: HashMap<String, GenerationModelConfig>,
}

impl ModelService {
    pub fn new(models: HashMap<String, GenerationModelConfig>) -> Self {
        Self { models }
    }

    pub fn find(
        &self,
        input_type: &ModalityType,
        output_type: &ModalityType,
    ) -> Option<(&str, &GenerationModelConfig)> {
        self.models
            .iter()
            .find(|(_, cfg)| {
                cfg.enabled && cfg.input_type == *input_type && cfg.output_type == *output_type
            })
            .map(|(name, cfg)| (name.as_str(), cfg))
    }

    pub fn all_enabled(&self) -> impl Iterator<Item = (&str, &GenerationModelConfig)> {
        self.models
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(name, cfg)| (name.as_str(), cfg))
    }
}
