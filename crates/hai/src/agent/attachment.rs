use async_trait::async_trait;

use crate::{
    agent::channel::ChannelHandle,
    domain::{model::Attachment, service::Services, vo::AttachmentId},
    error::Result,
};

/// The row lookup and platform bytes/URL one attachment needs, shared by read analysis and media
/// tools.
#[async_trait]
pub(crate) trait AttachmentSource: Send + Sync {
    async fn attachment(&self, id: AttachmentId) -> Result<Option<Attachment>>;
    async fn download(&self, file_id: &str) -> Result<Vec<u8>>;
    async fn file_url(&self, file_id: &str) -> Result<String>;
}

/// The production attachment source: the message service and the chat's own platform handle.
pub(crate) struct ToolAttachments {
    services: Services,
    chat: ChannelHandle,
}

impl ToolAttachments {
    pub(crate) fn new(services: Services, chat: ChannelHandle) -> Self {
        Self { services, chat }
    }
}

#[async_trait]
impl AttachmentSource for ToolAttachments {
    async fn attachment(&self, id: AttachmentId) -> Result<Option<Attachment>> {
        self.services.message.attachment_by_id(id).await
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>> {
        self.chat.download_file(file_id).await
    }

    async fn file_url(&self, file_id: &str) -> Result<String> {
        self.chat.file_url(file_id).await
    }
}
