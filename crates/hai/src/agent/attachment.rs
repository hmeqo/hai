use std::sync::Arc;

use uuid::Uuid;

use crate::{
    agent::node::{MediaInput, MultimodalService},
    bot::telegram::TelegramService,
    domain::{
        entity::Platform,
        service::DbServices,
        vo::{AttachmentParser, Source, TelegramContentPart},
    },
    error::{ErrorKind, OptionAppExt, Result},
    infra::cache::FileCache,
};

#[derive(Debug, Clone)]
pub struct AttachmentService(Arc<AttachmentServiceInner>);

#[derive(Debug)]
struct AttachmentServiceInner {
    cache: FileCache,
    telegram_file: TelegramService,
    db_srv: DbServices,
    multimodal: MultimodalService,
}

impl AttachmentService {
    pub fn new(
        cache: FileCache,
        telegram_file: TelegramService,
        db_srv: DbServices,
        multimodal: MultimodalService,
    ) -> Self {
        Self(Arc::new(AttachmentServiceInner {
            cache,
            telegram_file,
            db_srv,
            multimodal,
        }))
    }

    /// 下载 Telegram 文件，带磁盘缓存。
    async fn download_telegram(&self, file_id: &str) -> Result<Vec<u8>> {
        let cache_key = format!("{}-{}", Platform::Telegram, file_id);
        if let Some(cached) = self.0.cache.find(&cache_key) {
            return Ok(cached);
        }
        let data = self.0.telegram_file.download(file_id).await?;
        self.0.cache.add(&cache_key, &data)?;
        Ok(data)
    }

    /// 执行分析并将结果写入 perception 缓存。
    async fn with_perception_cache(
        &self,
        source: Source,
        parser: AttachmentParser,
        prompt: Option<&str>,
        analyze: impl std::future::Future<Output = Result<String>>,
    ) -> Result<String> {
        let content = analyze.await?;
        self.0
            .db_srv
            .perception
            .upsert(&source, parser.name(), prompt, &content)
            .await?;
        Ok(content)
    }

    /// 分析消息附件（通过 attachment_id 关联）。
    pub async fn analyze_attachment(
        &self,
        attachment_uuid: Uuid,
        prompt: Option<&str>,
    ) -> Result<String> {
        let (_, part) = self
            .0
            .db_srv
            .message
            .find_by_attachment_id(attachment_uuid)
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

        let source = Source::platform("telegram", &file_id);
        self.with_perception_cache(source, parser, prompt, async move {
            self.analyze_part(&part, &file_id, parser, prompt).await
        })
        .await
    }

    /// 根据消息 part 类型调用对应的多模态分析。
    async fn analyze_part(
        &self,
        part: &TelegramContentPart,
        file_id: &str,
        parser: AttachmentParser,
        prompt: Option<&str>,
    ) -> Result<String> {
        match parser {
            AttachmentParser::Image if matches!(part, TelegramContentPart::Sticker { .. }) => {
                let data = self.download_telegram(file_id).await?;
                self.0
                    .multimodal
                    .analyze_image(MediaInput::from_bytes(data, None), prompt)
                    .await
            }
            AttachmentParser::Image => {
                let url = self.0.telegram_file.get_file_url(file_id).await?;
                self.0
                    .multimodal
                    .analyze_image(MediaInput::from_url(url, None), prompt)
                    .await
            }
            AttachmentParser::Ocr => {
                let url = self.0.telegram_file.get_file_url(file_id).await?;
                self.0.multimodal.ocr(MediaInput::from_url(url, None)).await
            }
            AttachmentParser::Video => {
                let url = self.0.telegram_file.get_file_url(file_id).await?;
                self.0
                    .multimodal
                    .analyze_video(MediaInput::from_url(url, part.media_format()), prompt)
                    .await
            }
            AttachmentParser::Audio => {
                let data = self.download_telegram(file_id).await?;
                self.0
                    .multimodal
                    .analyze_audio(MediaInput::from_bytes(data, part.media_format()), prompt)
                    .await
            }
        }
    }

    /// 直接分析 URL 资源，不下载（图片、视频、音频链接等）。
    pub async fn analyze_url(
        &self,
        url: &str,
        parser: AttachmentParser,
        prompt: Option<&str>,
    ) -> Result<String> {
        let source = Source::url(url);
        let url = url.to_owned();
        self.with_perception_cache(source, parser, prompt, async move {
            match parser {
                AttachmentParser::Image => {
                    self.0
                        .multimodal
                        .analyze_image(MediaInput::from_url(url, None), prompt)
                        .await
                }
                AttachmentParser::Ocr => {
                    self.0.multimodal.ocr(MediaInput::from_url(url, None)).await
                }
                AttachmentParser::Video => {
                    self.0
                        .multimodal
                        .analyze_video(MediaInput::from_url(url, None), prompt)
                        .await
                }
                AttachmentParser::Audio => {
                    self.0
                        .multimodal
                        .analyze_audio(MediaInput::from_url(url, None), prompt)
                        .await
                }
            }
        })
        .await
    }
}
