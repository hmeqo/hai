mod catalog;
mod provider;
mod role;

pub(crate) use catalog::{ModelCatalog, ModelEntry};
pub(crate) use provider::ProviderRegistry;
pub(crate) use role::{ModelRoles, embedding, resolve};

use crate::error::{AppError, Result};

/// The two halves of a configured model id.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModelRef<'a> {
    pub provider: &'a str,
    pub model: &'a str,
}

/// Splits `provider/model` at the first slash; model ids may contain further slashes.
pub(crate) fn split_model(raw: &str) -> Result<ModelRef<'_>> {
    let (provider, model) = raw
        .split_once('/')
        .ok_or_else(|| AppError::message(format!("model {raw:?} must be provider/model")))?;
    if provider.is_empty() || model.is_empty() {
        return Err(AppError::message(format!(
            "model {raw:?} must name both provider and model"
        )));
    }
    Ok(ModelRef { provider, model })
}

/// What a resolved chat model can take besides text.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ModelCapabilities {
    pub image_input: bool,
}
