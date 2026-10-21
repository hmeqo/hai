use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

use super::media::{ImageFormat, MediaCodec};

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AttachmentParser {
    Image,
    Audio,
    Video,
    Ocr,
}

impl AttachmentParser {
    pub fn name(&self) -> &'static str {
        self.into()
    }

    /// The parser a byte prefix declares: image magic first, then container magic.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if ImageFormat::sniff(bytes).is_some() {
            return Some(Self::Image);
        }
        MediaCodec::sniff(bytes).map(Self::of_codec)
    }

    /// The parser a file name declares, by extension.
    pub fn from_filename(name: &str) -> Option<Self> {
        if ImageFormat::from_filename(name).is_some() {
            return Some(Self::Image);
        }
        MediaCodec::from_filename(name).map(Self::of_codec)
    }

    /// The parser that reads a container of this codec.
    fn of_codec(codec: MediaCodec) -> Self {
        if codec.is_audio() {
            Self::Audio
        } else {
            Self::Video
        }
    }
}

/// What one attachment row is; the name is the `<attachment type="…">` value and the column value.
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Photo,
    Video,
    Audio,
    Voice,
    Document,
    Sticker,
    Animation,
    VideoNote,
}

impl AttachmentKind {
    pub fn name(&self) -> &'static str {
        self.into()
    }
}

enum_column!(AttachmentKind);
enum_column!(AttachmentParser);

/// Platform extras carried by one attachment rather than by the message.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachmentMeta {
    /// The text an app-generated attachment was produced from (speech script, image prompt).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// A sticker's own emoji.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The codec the platform's own media type declared, when it declared one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_codec: Option<MediaCodec>,
}

impl sqlx::Type<sqlx::Postgres> for AttachmentMeta {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <serde_json::Value as sqlx::Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <serde_json::Value as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for AttachmentMeta {
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
        let raw = <serde_json::Value as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        serde_json::from_value(raw)
            .map_err(|e| format!("attachment meta cannot be read: {e}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_parser_reads_bytes_before_names() {
        assert_eq!(
            AttachmentParser::from_bytes(b"\x00\x00\x00\x18ftypisomrest"),
            Some(AttachmentParser::Video)
        );
        assert_eq!(
            AttachmentParser::from_bytes(b"\x89PNG\r\n\x1a\nrest"),
            Some(AttachmentParser::Image)
        );
        assert_eq!(
            AttachmentParser::from_bytes(b"OggS\x00\x02rest"),
            Some(AttachmentParser::Audio)
        );
        // A PNG named `a.mp4`: the caller tries `from_bytes` first, so content wins.
        assert_eq!(
            AttachmentParser::from_filename("a.mp4"),
            Some(AttachmentParser::Video)
        );
    }

    #[test]
    fn attachment_parser_from_filename_maps_the_media_extensions() {
        assert_eq!(
            AttachmentParser::from_filename("a.png"),
            Some(AttachmentParser::Image)
        );
        assert_eq!(
            AttachmentParser::from_filename("a.mp3"),
            Some(AttachmentParser::Audio)
        );
        assert_eq!(
            AttachmentParser::from_filename("a.ogg"),
            Some(AttachmentParser::Audio)
        );
        assert_eq!(
            AttachmentParser::from_filename("clip.mov"),
            Some(AttachmentParser::Video)
        );
        assert_eq!(AttachmentParser::from_filename("notes.txt"), None);
        assert_eq!(AttachmentParser::from_filename("no-extension"), None);
    }
}
