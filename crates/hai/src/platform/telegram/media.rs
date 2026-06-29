use uuid::Uuid;

use super::TelegramService;
use crate::{
    agent::node::MediaInput,
    app::AppContext,
    domain::{
        model::Platform,
        vo::{AttachmentParser, Source, TelegramContentPart},
    },
    error::{ErrorKind, OptionAppExt, Result},
    infra::cache::FileCache,
};

/// Telegram 媒体分析层：文件缓存 + 附件解析 + multimodal 分发
pub struct TelegramMediaAnalyzer {
    srv: TelegramService,
    cache: FileCache,
    ctx: AppContext,
}

impl TelegramMediaAnalyzer {
    pub fn new(srv: TelegramService, ctx: AppContext) -> Self {
        Self {
            srv,
            cache: FileCache::new(),
            ctx,
        }
    }

    pub async fn resolve_attachment(
        &self,
        attachment_uuid: Uuid,
    ) -> Result<(TelegramContentPart, String, AttachmentParser)> {
        let (_, part) = self
            .ctx
            .db
            .srv
            .message
            .find_attachment(attachment_uuid)
            .await?
            .ok_or_err_msg(
                ErrorKind::NotFound,
                format!("attachment_id {attachment_uuid} 不存在"),
            )?;

        let parser = part
            .attachment_parser()
            .ok_or_err_msg(ErrorKind::BadRequest, "该附件类型不支持解析")?;
        let file_id = part
            .file_id()
            .ok_or_err_msg(ErrorKind::BadRequest, "附件缺少 file_id")?
            .to_string();

        tracing::info!(
            attachment_uuid = %attachment_uuid,
            parser = %parser,
            file_id = %file_id,
            "analyze_attachment"
        );

        Ok((part, file_id, parser))
    }

    pub async fn analyze_part(
        &self,
        part: &TelegramContentPart,
        file_id: &str,
        parser: AttachmentParser,
        prompt: Option<&str>,
    ) -> Result<String> {
        match parser {
            AttachmentParser::Image if matches!(part, TelegramContentPart::Sticker { .. }) => {
                let data = self.download_file_cached(file_id).await?;
                self.ctx
                    .provider
                    .multimodal
                    .analyze_image(MediaInput::from_bytes(data, None), prompt)
                    .await
            }
            AttachmentParser::Image => {
                let url = self.file_url(file_id).await?;
                self.ctx
                    .provider
                    .multimodal
                    .analyze_image(MediaInput::from_url(url, None), prompt)
                    .await
            }
            AttachmentParser::Ocr => {
                let url = self.file_url(file_id).await?;
                self.ctx
                    .provider
                    .multimodal
                    .ocr(MediaInput::from_url(url, None))
                    .await
            }
            AttachmentParser::Video => {
                let url = self.file_url(file_id).await?;
                self.ctx
                    .provider
                    .multimodal
                    .analyze_video(MediaInput::from_url(url, part.media_format()), prompt)
                    .await
            }
            AttachmentParser::Audio => {
                let data = self.download_file_cached(file_id).await?;
                self.ctx
                    .provider
                    .multimodal
                    .analyze_audio(MediaInput::from_bytes(data, part.media_format()), prompt)
                    .await
            }
        }
    }

    pub async fn persist_analysis(
        &self,
        file_id: &str,
        parser: AttachmentParser,
        prompt: Option<&str>,
        content: &str,
    ) -> Result<()> {
        let source = Source::platform("telegram", file_id);
        self.ctx
            .db
            .srv
            .perception
            .upsert(&source, parser.name(), prompt, content)
            .await?;
        Ok(())
    }

    pub(crate) async fn download_file_cached(&self, file_id: &str) -> Result<Vec<u8>> {
        let cache_key = format!("{}-{}", Platform::Telegram, file_id);
        if let Some(cached) = self.cache.find(&cache_key) {
            return Ok(cached);
        }
        let data = self.srv.download(file_id).await?;
        self.cache.add(&cache_key, &data)?;
        Ok(data)
    }

    pub(crate) async fn file_url(&self, file_id: &str) -> Result<String> {
        self.srv.get_file_url(file_id).await
    }
}
