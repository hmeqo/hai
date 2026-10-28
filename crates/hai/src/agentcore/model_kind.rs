//! What a ModelRole asks a provider for, and the target that model kind resolves to.

use std::sync::Arc;

use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, IntoStaticStr};

/// What a ModelRole asks a provider for; `Chat` runs the Step loop.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Display,
    EnumIter,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ModelKind {
    Chat,
    Vision,
    Transcription,
    Embedding,
    Image,
    Speech,
    Rerank,
}

/// A resolved API key: the `Authorization: Bearer` header the OpenAI-compatible `ApiClient` endpoints
/// take, and the bare key the genai chat path hands its adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    /// The name and value of the request header this credential carries.
    pub fn header(&self) -> (&'static str, String) {
        ("authorization", format!("Bearer {}", self.0))
    }

    /// The bare key, for a client that carries it itself (the genai chat transport).
    pub fn key(&self) -> &str {
        &self.0
    }
}

/// A provider's own answer for one model kind: where it lives, when that is not the provider's base
/// URL.
#[derive(Debug, Clone, Copy)]
pub struct ModelKindSpec {
    pub url: Option<&'static str>,
}

/// One model call's resolved target; the provider headers are parsed once, at startup.
#[derive(Debug, Clone)]
pub struct ModelTarget {
    pub base_url: String,
    pub model: String,
    pub credential: Credential,
    pub headers: Arc<HeaderMap>,
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::ModelKind;
    use crate::test_support::assert_names_agree;

    /// The serde tag and the strum name are one spelling, pinned wherever both are derived.
    #[test]
    fn wire_form_and_strum_names_agree() {
        assert_names_agree(ModelKind::iter(), |kind| (*kind).into());
    }
}
