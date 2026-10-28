use super::{Read, ReadOutput};
use crate::{
    agent::multimodal::{
        ImageSource, Payload,
        analysis::{ResolvedMedia, reads_natively},
    },
    agentcore::tool::{ToolError, ToolMedia},
    domain::vo::{AttachmentId, AttachmentParser},
};

impl Read {
    pub(super) async fn read_attachment(
        &self,
        id: AttachmentId,
        parser: Option<AttachmentParser>,
        focus: Option<&str>,
        image_input: bool,
    ) -> Result<ReadOutput, ToolError> {
        let ResolvedMedia {
            parser,
            payload,
            subject,
        } = self.resolver.attachment(id, parser, image_input).await?;
        match payload {
            Payload::Image(ImageSource::Bytes { data, format })
                if reads_natively(parser, image_input) =>
            {
                Ok(ReadOutput::Image {
                    media: ToolMedia::Image {
                        mime: format.mime().to_owned(),
                        data,
                        focus: Some(focus.unwrap_or_default().trim().to_owned()),
                    },
                })
            }
            payload => {
                let content = self
                    .analyzer
                    .analyze(
                        ResolvedMedia {
                            parser,
                            payload,
                            subject,
                        },
                        focus,
                    )
                    .await?;
                Ok(ReadOutput::Text { content })
            }
        }
    }
}
