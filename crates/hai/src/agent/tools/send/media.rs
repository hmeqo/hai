//! What a `source` resolves to: the bytes of a local or attachment address, or the URL the platform
//! fetches itself.

use super::SendMessage;
use crate::{
    agent::{
        channel::{OutboundPayload, OutboundSource},
        tools::{addressing::Address, source::ResolvedSource},
    },
    agentcore::tool::ToolError,
    domain::vo::AttachmentParser,
};

impl SendMessage {
    pub(super) async fn media_payload(
        &self,
        source: Address,
        content: Option<String>,
    ) -> Result<OutboundPayload, ToolError> {
        let (media, filename) = self.media_source(source).await?;
        let sniffed = match &media {
            OutboundSource::Bytes(bytes) => AttachmentParser::from_bytes(bytes),
            OutboundSource::Url(_) => None,
        };
        let is_image = sniffed.or_else(|| AttachmentParser::from_filename(&filename))
            == Some(AttachmentParser::Image);
        if is_image {
            Ok(OutboundPayload::Photo {
                source: media,
                content,
            })
        } else {
            Ok(OutboundPayload::Document {
                source: media,
                filename,
                content,
            })
        }
    }

    /// The bytes of a local or attachment address, or the URL the platform fetches itself; either
    /// way with the name to fall back on.
    async fn media_source(&self, source: Address) -> Result<(OutboundSource, String), ToolError> {
        match self
            .resolver
            .resolve(&source, self.capabilities.max_media_bytes)
            .await?
        {
            ResolvedSource::Bytes { data, name } => Ok((OutboundSource::Bytes(data), name)),
            ResolvedSource::Url { url, name } => Ok((OutboundSource::Url(url), name)),
        }
    }
}
