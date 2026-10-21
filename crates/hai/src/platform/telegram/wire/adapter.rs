use std::{fmt, time::Duration};

use teloxide::{
    Bot,
    payloads::{
        AnswerCallbackQuerySetters, EditMessageTextSetters, SendDocumentSetters,
        SendMessageSetters, SendPhotoSetters, SendVoiceSetters,
    },
    prelude::Requester,
    types::{
        CallbackQueryId, ChatId as TgChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile,
        MessageId as TgMessageId, ReplyMarkup, ReplyParameters,
    },
};

use super::super::{
    config::TelegramConfig,
    outbound::{
        receipt::{SentMedia, receipt, sent_attachment},
        reply,
        service::{self, TelegramFiles, TelegramService, telegram_err},
        typing::TelegramTyping,
    },
    ui::forms::{MAX_UPLOAD_BYTES, TELEGRAM_FORMS},
};
use crate::{
    agent::channel::{
        CallbackAnswer, ChannelAccess, ChannelCapabilities, ChannelProfile, ChoiceOption,
        DeleteReport, FailedDeletion, OutboundPayload, OutboundSource, TypingGuard,
    },
    domain::{
        model::Platform,
        vo::{AccountId, ChannelKey},
    },
    error::{AppError, Result},
    platform::adapter::{ChannelAdapter, SentMessage, WireMessage},
};

/// One `deleteMessages` call carries at most this many ids.
const DELETE_BATCH: usize = 100;

/// The pause between two calls: about twenty a second, inside the platform's own rate limit.
const DELETE_PACING: Duration = Duration::from_millis(50);

/// How many times a rate-limited batch is retried before its ids are attributed one by one.
const DELETE_RETRIES: u8 = 3;

/// What a Telegram channel can put in a chat: every form, replies that point at and quote, the
/// platform's upload ceiling, and the buttons a page of choices needs.
pub(in crate::platform::telegram) const TELEGRAM_CAPABILITIES: ChannelCapabilities =
    ChannelCapabilities {
        forms: &TELEGRAM_FORMS,
        reference: true,
        quote: true,
        choices: true,
        max_media_bytes: MAX_UPLOAD_BYTES,
    };

/// One media request: the caption when there is one, the reply parameters when there are any, then
/// the send. The caller has already cut the caption to the platform's ceiling. The three teloxide
/// setters share no trait, so the shared part is a macro.
macro_rules! media_request {
    ($request:expr, $content:expr, $reply_params:expr) => {{
        let mut request = $request;
        if let Some(content) = $content {
            request = request.caption(content);
        }
        if let Some(params) = $reply_params {
            request = request.reply_parameters(params);
        }
        request.await.map_err(telegram_err)?
    }};
}

/// The Telegram adapter: one channel's own settings, its client, and the send helpers that speak the
/// Bot API. Nothing here reads the store or the process container.
pub(in crate::platform::telegram) struct TelegramAdapter {
    bot: Bot,
    srv: TelegramService,
    files: TelegramFiles,
    key: ChannelKey,
    profile: ChannelProfile,
    allowed_chat_ids: Vec<i64>,
    rich_message: bool,
}

impl TelegramAdapter {
    pub(in crate::platform::telegram) async fn new(
        bot: Bot,
        key: ChannelKey,
        cfg: TelegramConfig,
        account_id: AccountId,
    ) -> Result<Self> {
        let me = bot.get_me().await.map_err(telegram_err)?;
        let name = bot.get_my_name().await.map_err(telegram_err)?.name;
        let profile = ChannelProfile {
            account_id,
            name,
            username: me.username.clone().unwrap_or_default(),
        };
        let srv = TelegramService::new(bot.clone());
        Ok(Self {
            files: TelegramFiles::new(srv.clone()),
            srv,
            bot,
            key,
            profile,
            allowed_chat_ids: cfg.allowed_chat_ids,
            rich_message: cfg.rich_message,
        })
    }

    /// The Telegram chat id behind one of the platform's own chat ids.
    fn chat_id(chat_external_id: &str) -> Result<TgChatId> {
        chat_external_id.parse::<i64>().map(TgChatId).map_err(|e| {
            AppError::message(format!("invalid Telegram chat id {chat_external_id}: {e}"))
        })
    }

    /// One of the platform's message ids, out of the string the store keeps it as.
    fn message_id(message_external_id: &str) -> Result<TgMessageId> {
        message_external_id
            .parse::<i32>()
            .map(TgMessageId)
            .map_err(|e| {
                AppError::message(format!(
                    "invalid Telegram message id {message_external_id}: {e}"
                ))
            })
    }

    /// Telegram rejects a photo caption longer than 1024 characters, so it is
    /// truncated before sending.
    fn truncate_caption(caption: &str) -> String {
        const CAPTION_MAX: usize = 1024;
        caption.chars().take(CAPTION_MAX).collect()
    }

    async fn send_with_rich_fallback(
        &self,
        content: &str,
        cid: TgChatId,
        reply_params: Option<&ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        match self.srv.send_rich_message(cid, content, reply_params).await {
            Ok(msg) => Ok(msg),
            Err(e) => {
                tracing::warn!(chat_id = cid.0, error = %e, "rich message send failed; falling back to text");
                service::send_markdown_with_fallback(&self.bot, cid, content, reply_params).await
            }
        }
    }

    async fn send_text(
        &self,
        cid: TgChatId,
        content: &str,
        reply_params: Option<ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        if self.rich_message {
            self.send_with_rich_fallback(content, cid, reply_params.as_ref())
                .await
        } else {
            service::send_markdown_with_fallback(&self.bot, cid, content, reply_params.as_ref())
                .await
        }
    }

    async fn send_voice(
        &self,
        cid: TgChatId,
        bytes: Vec<u8>,
        content: Option<&str>,
        reply_params: Option<ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        let request = self.bot.send_voice(cid, InputFile::memory(bytes));
        Ok(media_request!(request, content, reply_params))
    }

    async fn send_photo(
        &self,
        cid: TgChatId,
        source: OutboundSource,
        content: Option<&str>,
        reply_params: Option<ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        let request = self.bot.send_photo(cid, input_file(source));
        Ok(media_request!(request, content, reply_params))
    }

    async fn send_document(
        &self,
        cid: TgChatId,
        source: OutboundSource,
        filename: String,
        content: Option<&str>,
        reply_params: Option<ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        let request = self
            .bot
            .send_document(cid, input_file(source).file_name(filename));
        Ok(media_request!(request, content, reply_params))
    }
}

impl fmt::Debug for TelegramAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramAdapter").finish()
    }
}

#[async_trait::async_trait]
impl ChannelAccess for TelegramAdapter {
    fn admits(&self, chat_external_id: &str) -> bool {
        chat_external_id
            .parse::<i64>()
            .is_ok_and(|id| self.allowed_chat_ids.contains(&id))
    }

    fn command_prefix(&self) -> &'static str {
        "/"
    }

    /// One message the host composes itself: delivered, never stored, and never pointing at anything.
    async fn notice(&self, chat_external_id: &str, text: &str) -> Result<()> {
        let cid = Self::chat_id(chat_external_id)?;
        service::send_markdown_with_fallback(&self.bot, cid, text, None).await?;
        Ok(())
    }

    /// A question with buttons under it. No parse mode: the copy carries punctuation markdown would
    /// have to escape, and a question is no place to discover that a character was special.
    async fn ask(
        &self,
        chat_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()> {
        let cid = Self::chat_id(chat_external_id)?;
        self.bot
            .send_message(cid, prompt)
            .reply_markup(ReplyMarkup::InlineKeyboard(keyboard(options)))
            .await
            .map_err(telegram_err)?;
        Ok(())
    }

    /// Rewrites the host's own question in place, which is how a page turns into the next one without
    /// the chat filling up with them.
    async fn edit_choice(
        &self,
        chat_external_id: &str,
        message_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()> {
        let cid = Self::chat_id(chat_external_id)?;
        self.bot
            .edit_message_text(cid, Self::message_id(message_external_id)?, prompt)
            .reply_markup(keyboard(options))
            .await
            .map_err(telegram_err)?;
        Ok(())
    }

    /// The answer is what stops the client waiting; `Quiet` shows nothing at all, which is what a
    /// page that already redrew itself needs, while `Alert` is a popup the person has to dismiss.
    async fn answer_callback(&self, query_id: &str, answer: CallbackAnswer<'_>) -> Result<()> {
        let request = self
            .bot
            .answer_callback_query(CallbackQueryId(query_id.to_owned()));
        let request = match answer {
            CallbackAnswer::Quiet => request,
            CallbackAnswer::Toast(text) | CallbackAnswer::Alert(text) => {
                request.text(text.to_owned())
            }
        };
        let request = match answer {
            CallbackAnswer::Alert(_) => request.show_alert(true),
            _ => request,
        };
        request.await.map_err(telegram_err)?;
        Ok(())
    }

    /// Takes the host's own page away once its report has arrived: the chat keeps the document, not
    /// the status message that led to it.
    async fn delete_notice(&self, chat_external_id: &str, message_external_id: &str) -> Result<()> {
        let report = self
            .delete_messages(chat_external_id, &[message_external_id.to_owned()])
            .await?;
        for failure in &report.failed {
            tracing::warn!(
                external_id = %failure.external_id,
                detail = %failure.detail,
                "host page not removed",
            );
        }
        Ok(())
    }

    /// A file the host composed itself: it goes out as a document with a name, and nothing about it
    /// is recorded — host copy has no row.
    async fn notice_document(
        &self,
        chat_external_id: &str,
        filename: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()> {
        let cid = Self::chat_id(chat_external_id)?;
        let caption = Self::truncate_caption(caption);
        self.send_document(
            cid,
            OutboundSource::Bytes(bytes),
            filename.to_owned(),
            Some(&caption),
            None,
        )
        .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    fn key(&self) -> &ChannelKey {
        &self.key
    }

    fn profile(&self) -> &ChannelProfile {
        &self.profile
    }

    fn capabilities(&self) -> ChannelCapabilities {
        TELEGRAM_CAPABILITIES
    }

    async fn send(&self, msg: WireMessage) -> Result<SentMessage> {
        let WireMessage {
            chat_external_id,
            reference,
            payload,
        } = msg;
        let cid = Self::chat_id(&chat_external_id)?;
        let reference = reference.as_ref();
        let reply_params = match reference {
            Some(reference) => Some(reply::build_reply(
                &reference.external_id,
                reference.excerpt.as_deref(),
            )?),
            None => None,
        };

        match payload {
            OutboundPayload::Text { content } => {
                let sent = self.send_text(cid, &content, reply_params).await?;
                receipt(&sent, reference, Vec::new())
            }
            OutboundPayload::Voice {
                bytes,
                content,
                prompt,
            } => {
                // The caption is cut to the platform's ceiling once, so the receipt records the line
                // the chat actually received.
                let content = content.as_deref().map(Self::truncate_caption);
                let sent = self
                    .send_voice(cid, bytes, content.as_deref(), reply_params)
                    .await?;
                receipt(
                    &sent,
                    reference,
                    vec![sent_attachment(
                        SentMedia::Voice,
                        &sent,
                        content.as_deref(),
                        Some(prompt),
                    )],
                )
            }
            OutboundPayload::Photo { source, content } => {
                let content = content.as_deref().map(Self::truncate_caption);
                let sent = self
                    .send_photo(cid, source, content.as_deref(), reply_params)
                    .await?;
                receipt(
                    &sent,
                    reference,
                    vec![sent_attachment(
                        SentMedia::Photo,
                        &sent,
                        content.as_deref(),
                        None,
                    )],
                )
            }
            OutboundPayload::Document {
                source,
                filename,
                content,
            } => {
                let content = content.as_deref().map(Self::truncate_caption);
                let sent = self
                    .send_document(cid, source, filename, content.as_deref(), reply_params)
                    .await?;
                receipt(
                    &sent,
                    reference,
                    vec![sent_attachment(
                        SentMedia::Document,
                        &sent,
                        content.as_deref(),
                        None,
                    )],
                )
            }
        }
    }

    /// Deleting is best effort: ids the platform refuses are reported per message, and the batch
    /// they were part of is attributed one by one so one dead id does not cost the others.
    async fn delete_messages(
        &self,
        chat_external_id: &str,
        ids: &[String],
    ) -> Result<DeleteReport> {
        let cid = Self::chat_id(chat_external_id)?;
        let mut report = DeleteReport::default();
        let (parsable, unparsable): (Vec<&String>, Vec<&String>) =
            ids.iter().partition(|id| id.parse::<i32>().is_ok());
        for id in unparsable {
            report.failed.push(FailedDeletion {
                external_id: id.clone(),
                detail: "not a Telegram message id".to_owned(),
            });
        }

        for batch in parsable.chunks(DELETE_BATCH) {
            let batch: Vec<TgMessageId> = batch
                .iter()
                .filter_map(|id| id.parse::<i32>().ok().map(TgMessageId))
                .collect();
            let mut attempt = 0;
            loop {
                match self.bot.delete_messages(cid, batch.clone()).await {
                    Ok(_) => {
                        report.deleted += batch.len() as u64;
                        break;
                    }
                    Err(teloxide::RequestError::RetryAfter(after)) if attempt < DELETE_RETRIES => {
                        attempt += 1;
                        tokio::time::sleep(after.duration()).await;
                    }
                    Err(_) => {
                        // The batch as a whole was refused, which says nothing about which id was
                        // the reason: each is asked for on its own.
                        for id in &batch {
                            match self.bot.delete_message(cid, *id).await {
                                Ok(_) => report.deleted += 1,
                                Err(e) => report.failed.push(FailedDeletion {
                                    external_id: id.0.to_string(),
                                    detail: e.to_string(),
                                }),
                            }
                        }
                        break;
                    }
                }
            }
            tokio::time::sleep(DELETE_PACING).await;
        }
        Ok(report)
    }

    async fn forget_cached(&self, file_ids: &[String]) -> Result<u64> {
        self.files.forget_cached(file_ids).await
    }

    fn start_typing(&self, chat_external_id: &str) -> Option<Box<dyn TypingGuard>> {
        let cid = match Self::chat_id(chat_external_id) {
            Ok(cid) => cid,
            Err(e) => {
                tracing::error!("Failed to send typing message: {e}");
                return None;
            }
        };
        Some(Box::new(TelegramTyping::start(self.bot.clone(), cid)))
    }

    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        self.files.download_file_cached(file_id).await
    }

    async fn file_url(&self, file_id: &str) -> Result<String> {
        self.srv.file_url(file_id).await
    }
}

fn input_file(source: OutboundSource) -> InputFile {
    match source {
        OutboundSource::Bytes(bytes) => InputFile::memory(bytes),
        OutboundSource::Url(url) => InputFile::url(url.into_url()),
    }
}

/// The buttons as this platform lays them out: two to a row, in the order they were given. No
/// options is an empty keyboard, which is how Telegram is told to take one away.
fn keyboard(options: &[ChoiceOption]) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(options.chunks(2).map(|row| {
        row.iter()
            .map(|option| {
                InlineKeyboardButton::callback(option.label.clone(), option.token.clone())
            })
            .collect::<Vec<_>>()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caption_is_cut_at_the_platform_ceiling() {
        let at_limit = "x".repeat(1024);
        assert_eq!(TelegramAdapter::truncate_caption(&at_limit), at_limit);

        let over = format!("{at_limit}tail");
        assert_eq!(TelegramAdapter::truncate_caption(&over), at_limit);

        // Cut by characters, never by bytes: a caption of one multi-byte character per unit survives
        // the cut whole.
        let multi_byte = "字".repeat(1025);
        let cut = TelegramAdapter::truncate_caption(&multi_byte);
        assert_eq!(cut.chars().count(), 1024);
        assert_eq!(cut, "字".repeat(1024));
    }

    /// The layout is this platform's: two to a row in the given order, and no options at all is the
    /// empty keyboard that takes a keyboard away.
    #[test]
    fn buttons_are_laid_out_two_to_a_row() {
        let option = |label: &str| ChoiceOption {
            label: label.to_owned(),
            token: format!("token-{label}"),
        };

        let markup = keyboard(&[option("a"), option("b"), option("c")]);
        let rows: Vec<Vec<&str>> = markup
            .inline_keyboard
            .iter()
            .map(|row| {
                row.iter()
                    .map(|button| button.text.as_str())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["c"]]);

        assert!(
            keyboard(&[]).inline_keyboard.is_empty(),
            "a page with no buttons sends an empty keyboard"
        );
    }
}
