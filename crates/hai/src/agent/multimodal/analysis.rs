//! Two pipelines: resolving a source into a payload, and analyzing that payload with the perception
//! cache.

use std::sync::Arc;

use crate::{
    agent::{
        attachment::AttachmentSource,
        multimodal::{ImageSource, MediaSource, MultimodalService, Payload},
    },
    domain::{
        service::Services,
        vo::{
            AttachmentId, AttachmentKind, AttachmentParser, ConversationId, ImageFormat, Subject,
        },
    },
    error::{AppError, Result},
};

/// A payload plus the parser that reads it and the cache key it may have.
pub(crate) struct ResolvedMedia {
    pub parser: AttachmentParser,
    pub payload: Payload,
    pub subject: Option<Subject>,
}

/// Resolves an attachment, local file or URL into bytes or a URL; it does not call a model.
#[derive(Clone)]
pub struct MediaResolver {
    attachments: Arc<dyn AttachmentSource>,
    inline_limit: InlineLimit,
}

impl MediaResolver {
    pub(crate) fn new(attachments: Arc<dyn AttachmentSource>, inline_limit: InlineLimit) -> Self {
        Self {
            attachments,
            inline_limit,
        }
    }

    /// The ceiling a caller refuses a file with before reading it.
    pub(crate) fn inline_limit(&self) -> InlineLimit {
        self.inline_limit
    }

    /// The attachment path: the row decides parser and subject, the platform the payload.
    pub async fn attachment(
        &self,
        id: AttachmentId,
        parser_override: Option<AttachmentParser>,
        image_input: bool,
    ) -> Result<ResolvedMedia> {
        let attachment = self
            .attachments
            .attachment(id)
            .await?
            .ok_or_else(|| AppError::message(format!("no attachment has id {id}")))?;
        let detected = attachment.parser().ok_or_else(|| {
            AppError::message(format!(
                "this file cannot be analyzed: {}",
                attachment
                    .meta
                    .file_name
                    .as_deref()
                    .unwrap_or(&attachment.file_id)
            ))
        })?;
        let parser = resolve_parser(detected, parser_override)?;
        let payload = match (parser, attachment.kind) {
            (AttachmentParser::Audio, _) => Payload::Media(
                MediaSource::Bytes(self.attachments.download(&attachment.file_id).await?),
                attachment.media_format(),
            ),
            (AttachmentParser::Video, _) => Payload::Media(
                MediaSource::Url(self.attachments.file_url(&attachment.file_id).await?),
                attachment.media_format(),
            ),
            (AttachmentParser::Image, AttachmentKind::Sticker) => {
                let data = self.attachments.download(&attachment.file_id).await?;
                let format = ImageFormat::sniff(&data).ok_or_else(|| {
                    AppError::message("sticker is not an image the vision model can read")
                })?;
                Payload::Image(ImageSource::bytes(data, format))
            }
            (AttachmentParser::Image, _) if reads_natively(parser, image_input) => {
                let data = self.attachments.download(&attachment.file_id).await?;
                let format = ImageFormat::sniff(&data).ok_or_else(|| {
                    AppError::message("attachment is not an image the model can read")
                })?;
                Payload::Image(ImageSource::bytes(data, format))
            }
            (AttachmentParser::Image | AttachmentParser::Ocr, _) => Payload::Image(
                ImageSource::Url(self.attachments.file_url(&attachment.file_id).await?),
            ),
        };
        check_inline_size(self.inline_limit, &payload)?;
        Ok(ResolvedMedia {
            parser,
            payload,
            subject: Some(attachment.subject()),
        })
    }
}

/// The one place a payload is analyzed: the result is cached under its subject.
#[derive(Clone)]
pub struct MediaAnalyzer {
    multimodal: MultimodalService,
    services: Services,
    conversation_id: ConversationId,
    inline_limit: InlineLimit,
}

impl MediaAnalyzer {
    pub(crate) fn new(
        multimodal: MultimodalService,
        services: Services,
        conversation_id: ConversationId,
        inline_limit: InlineLimit,
    ) -> Self {
        Self {
            multimodal,
            services,
            conversation_id,
            inline_limit,
        }
    }

    /// Analyzes `resolved` and caches the result under its subject; no subject means no cache.
    pub async fn analyze(&self, resolved: ResolvedMedia, focus: Option<&str>) -> Result<String> {
        let focus = focus_of(focus);
        let ResolvedMedia {
            parser,
            payload,
            subject,
        } = resolved;
        check_inline_size(self.inline_limit, &payload)?;
        if let Some(subject) = &subject
            && let Some(hit) = self
                .services
                .perception
                .find(self.conversation_id, subject, parser, focus)
                .await?
        {
            return Ok(hit.content);
        }
        let content = self.multimodal.analyze(parser, payload, focus).await?;
        if let Some(subject) = &subject {
            self.services
                .perception
                .upsert(self.conversation_id, subject, parser, focus, &content)
                .await?;
        }
        Ok(content)
    }
}

/// Bytes handed to a model are bounded here; `analyze` and `attachment` (whose direct-image path
/// returns without analyzing) both apply it.
fn check_inline_size(limit: InlineLimit, payload: &Payload) -> Result<()> {
    let len = match payload {
        Payload::Image(ImageSource::Bytes { data, .. }) => data.len(),
        Payload::Media(MediaSource::Bytes(data), _) => data.len(),
        Payload::Image(ImageSource::Url(_)) | Payload::Media(MediaSource::Url(_), _) => {
            return Ok(());
        }
    };
    limit.refuse(len as u64)
}

/// An empty instruction is no instruction: the cache key and the prompt both treat it as absent.
fn focus_of(focus: Option<&str>) -> Option<&str> {
    focus.map(str::trim).filter(|focus| !focus.is_empty())
}

/// The inline ceiling one request may carry; `0` leaves it unbounded.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InlineLimit(u64);

impl InlineLimit {
    pub(crate) fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// Refuses bytes over the limit before they are read or sent.
    pub(crate) fn refuse(&self, len: u64) -> Result<(), AppError> {
        if self.0 != 0 && len > self.0 {
            return Err(AppError::message(format!(
                "inline analysis is limited to {} MB",
                self.0 / (1024 * 1024)
            )));
        }
        Ok(())
    }
}

/// Whether the parser reads an image as image bytes for a model that can see, instead of text
/// analysis.
pub(crate) fn reads_natively(parser: AttachmentParser, image_input: bool) -> bool {
    image_input && parser == AttachmentParser::Image
}

/// The requested parser wins when it is another reading of the same payload.
pub(crate) fn resolve_parser(
    detected: AttachmentParser,
    requested: Option<AttachmentParser>,
) -> Result<AttachmentParser> {
    match requested {
        None => Ok(detected),
        Some(AttachmentParser::Ocr) if detected == AttachmentParser::Image => {
            Ok(AttachmentParser::Ocr)
        }
        Some(p) if p == detected => Ok(p),
        Some(p) => Err(AppError::message(format!(
            "parser `{p}` cannot read this file"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::{
        domain::{model::Attachment, vo::AttachmentKind},
        error::Result,
    };

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    struct FixedAttachments {
        row: Attachment,
        bytes: Vec<u8>,
        url: String,
    }

    #[async_trait]
    impl AttachmentSource for FixedAttachments {
        async fn attachment(&self, id: AttachmentId) -> Result<Option<Attachment>> {
            Ok((self.row.id == id).then(|| self.row.clone()))
        }

        async fn download(&self, _file_id: &str) -> Result<Vec<u8>> {
            Ok(self.bytes.clone())
        }

        async fn file_url(&self, _file_id: &str) -> Result<String> {
            Ok(self.url.clone())
        }
    }

    #[tokio::test]
    async fn an_attachment_resolves_through_the_shared_source() {
        let mut row = crate::domain::test_support::attachment(AttachmentKind::Photo);
        row.meta.file_name = Some("ref.png".into());
        let resolver = MediaResolver::new(
            Arc::new(FixedAttachments {
                row: row.clone(),
                bytes: PNG_MAGIC.to_vec(),
                url: "https://example.com/ref.png".into(),
            }),
            InlineLimit::new(0),
        );

        let resolved = resolver
            .attachment(row.id, None, true)
            .await
            .expect("attachment");
        assert_eq!(resolved.parser, AttachmentParser::Image);
        match resolved.payload {
            Payload::Image(ImageSource::Bytes { data, .. }) => assert_eq!(data, PNG_MAGIC),
            _ => panic!("expected image bytes"),
        }
    }

    #[test]
    fn an_override_switches_between_the_two_readings_of_an_image() {
        assert_eq!(
            resolve_parser(AttachmentParser::Image, None).unwrap(),
            AttachmentParser::Image
        );
        assert_eq!(
            resolve_parser(AttachmentParser::Image, Some(AttachmentParser::Ocr)).unwrap(),
            AttachmentParser::Ocr
        );
        assert_eq!(
            resolve_parser(AttachmentParser::Audio, Some(AttachmentParser::Audio)).unwrap(),
            AttachmentParser::Audio
        );
    }

    #[test]
    fn an_override_cannot_move_a_file_to_another_pipeline() {
        assert!(resolve_parser(AttachmentParser::Image, Some(AttachmentParser::Video)).is_err());
        assert!(resolve_parser(AttachmentParser::Video, Some(AttachmentParser::Audio)).is_err());
        assert!(resolve_parser(AttachmentParser::Audio, Some(AttachmentParser::Image)).is_err());
    }

    #[test]
    fn an_oversized_inline_payload_is_refused() {
        const MAX: u64 = 20 * 1024 * 1024;
        assert!(InlineLimit::new(MAX).refuse(16).is_ok());
        assert!(
            InlineLimit::new(0).refuse(MAX + 1).is_ok(),
            "0 is unbounded"
        );
        let error = InlineLimit::new(MAX).refuse(MAX + 1).expect_err("refused");
        assert!(error.to_string().contains("20 MB"), "{error}");
    }

    #[test]
    fn only_a_native_image_read_skips_the_analyzer() {
        assert!(reads_natively(AttachmentParser::Image, true));
        assert!(!reads_natively(AttachmentParser::Ocr, true));
        assert!(!reads_natively(AttachmentParser::Image, false));
        assert!(!reads_natively(AttachmentParser::Audio, true));
    }

    #[test]
    fn an_empty_focus_is_no_focus() {
        assert_eq!(focus_of(None), None);
        assert_eq!(focus_of(Some("")), None);
        assert_eq!(focus_of(Some("  ")), None);
        assert_eq!(focus_of(Some(" x ")), Some("x"));
    }
}
