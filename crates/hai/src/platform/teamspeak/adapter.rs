use std::{
    fmt,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use tokio::{io::AsyncWriteExt, time::timeout};
use tsclientlib::{ChannelId, MessageTarget, OutCommandExt, data, sync::SyncConnectionHandle};
use uuid::Uuid;

use super::inbound::ChatId;
use crate::{
    agent::channel::{
        BuildCapability, ChannelAccess, ChannelCapabilities, ChannelProfile, DeleteReport,
        FailedDeletion, OutboundField, OutboundForm, OutboundKind, OutboundPayload, TypingGuard,
    },
    domain::{
        model::Platform,
        vo::{ChannelKey, ChatAddress, ChatOrigin},
    },
    error::{AppError, Result},
    platform::adapter::{ChannelAdapter, SentMessage, WireMessage},
};

/// TeamSpeak's `sendtextmessage` ceiling, in UTF-8 bytes.
const MAX_TEXT_BYTES: usize = 1024;
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const UPLOAD_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK_PACING: Duration = Duration::from_millis(250);

const TEAMSPEAK_FORMS: [OutboundForm; 1] = [OutboundForm {
    kind: OutboundKind::Text,
    fields: &[OutboundField {
        name: "content",
        description: "What to say",
    }],
    requires: BuildCapability::None,
}];

const TEAMSPEAK_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
    forms: &TEAMSPEAK_FORMS,
    reference: false,
    quote: false,
    max_media_bytes: 0,
};

enum Session {
    Live {
        handle: SyncConnectionHandle,
        origin: Option<ChatOrigin>,
    },
    #[cfg(test)]
    TestOrigin { origin: ChatOrigin },
}

/// Keeps one TeamSpeak connection's session installed for a connection generation; dropping it
/// clears the session.
pub(super) struct SessionGuard<'a> {
    adapter: &'a TeamspeakAdapter,
}

impl SessionGuard<'_> {
    pub(super) fn joined(&self, origin: ChatOrigin) -> Result<()> {
        self.adapter.joined(origin)
    }
}

impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        self.adapter.end_session();
    }
}

pub(super) struct TeamspeakAdapter {
    key: ChannelKey,
    profile: ChannelProfile,
    allowed_chat_ids: Option<Vec<String>>,
    session: Mutex<Option<Session>>,
}

impl TeamspeakAdapter {
    pub(super) fn new(
        key: ChannelKey,
        profile: ChannelProfile,
        allowed_chat_ids: Option<Vec<String>>,
    ) -> Self {
        Self {
            key,
            profile,
            allowed_chat_ids,
            session: Mutex::new(None),
        }
    }

    pub(super) fn open(&self, handle: SyncConnectionHandle) -> SessionGuard<'_> {
        *self.session() = Some(Session::Live {
            handle,
            origin: None,
        });
        SessionGuard { adapter: self }
    }

    fn joined(&self, origin: ChatOrigin) -> Result<()> {
        let mut session = self.session();
        match session
            .as_mut()
            .ok_or_else(|| AppError::message("TeamSpeak session is not open"))?
        {
            Session::Live {
                origin: current, ..
            } => {
                *current = Some(origin);
                Ok(())
            }
            #[cfg(test)]
            Session::TestOrigin { .. } => Err(AppError::message("TeamSpeak session is not live")),
        }
    }

    fn end_session(&self) {
        *self.session() = None;
    }

    fn session(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn handle(&self) -> Result<SyncConnectionHandle> {
        match self.session().as_ref() {
            Some(Session::Live { handle, .. }) => Ok(handle.clone()),
            #[cfg(test)]
            Some(Session::TestOrigin { .. }) => {
                Err(AppError::message("TeamSpeak connection is not ready"))
            }
            None => Err(AppError::message("TeamSpeak connection is not ready")),
        }
    }

    fn origin(&self) -> Result<ChatOrigin> {
        match self.session().as_ref() {
            Some(Session::Live {
                origin: Some(origin),
                ..
            }) => Ok(origin.clone()),
            #[cfg(test)]
            Some(Session::TestOrigin { origin }) => Ok(origin.clone()),
            _ => Err(AppError::message("TeamSpeak server identity is not ready")),
        }
    }

    #[cfg(test)]
    fn test_origin(&self, origin: ChatOrigin) {
        *self.session() = Some(Session::TestOrigin { origin });
    }

    fn chat_from_address(&self, address: &ChatAddress) -> Result<ChatId> {
        if address.origin.as_ref() != Some(&self.origin()?) {
            return Err(AppError::message(format!(
                "chat '{}' belongs to another TeamSpeak server",
                address
            )));
        }
        address.external_id.parse().map_err(AppError::message)
    }

    async fn send_once(&self, chat: &ChatId, text: &str) -> Result<()> {
        let mut handle = self.handle()?;
        let chat = chat.clone();
        let text = text.to_owned();
        let result = timeout(
            SEND_TIMEOUT,
            handle.with_connection(move |con| {
                let command = {
                    let state = con
                        .get_state()
                        .map_err(|e| AppError::adapter("teamspeak", e))?;
                    let target = target_of(state, &chat)?;
                    state.send_message(target, &text)
                };
                command
                    .send(con)
                    .map_err(|e| AppError::adapter("teamspeak", e))
            }),
        )
        .await
        .map_err(|_| AppError::message("TeamSpeak send timed out"))?;
        result.map_err(|e| AppError::adapter("teamspeak", e))?
    }

    async fn send_chunks(&self, chat: &ChatId, text: &str) -> Result<()> {
        for (index, chunk) in chunks(text, MAX_TEXT_BYTES).into_iter().enumerate() {
            if index > 0 {
                tokio::time::sleep(CHUNK_PACING).await;
            }
            self.send_once(chat, chunk).await?;
        }
        Ok(())
    }

    fn private_report_text(caption: &str, bytes: Vec<u8>) -> Result<String> {
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::message("the TeamSpeak digest report is not UTF-8"))?;
        if caption.is_empty() {
            Ok(text)
        } else {
            Ok(format!("{caption}\n\n{text}"))
        }
    }
}

impl fmt::Debug for TeamspeakAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TeamspeakAdapter").finish()
    }
}

#[async_trait::async_trait]
impl ChannelAccess for TeamspeakAdapter {
    fn admits(&self, chat: &ChatAddress) -> bool {
        let Ok(origin) = self.origin() else {
            return false;
        };
        if chat.origin.as_ref() != Some(&origin) {
            return false;
        }
        match &self.allowed_chat_ids {
            None => true,
            Some(allowed) => allowed.iter().any(|id| id == &chat.external_id),
        }
    }

    fn private_chat_external_id(&self, sender_external_id: &str) -> Result<ChatAddress> {
        Ok(ChatAddress::new(
            Some(self.origin()?),
            format!("client:{sender_external_id}"),
        ))
    }

    fn command_prefix(&self) -> &'static str {
        "!"
    }

    async fn notice(&self, chat: &ChatAddress, text: &str) -> Result<()> {
        let chat = self.chat_from_address(chat)?;
        self.send_chunks(&chat, text).await
    }

    async fn notice_document(
        &self,
        chat: &ChatAddress,
        filename: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()> {
        let chat = self.chat_from_address(chat)?;
        let filename = sanitize_filename::sanitize(filename);
        if filename.is_empty() {
            return Err(AppError::message("TeamSpeak file name is empty"));
        }
        if matches!(chat, ChatId::Client(_)) {
            let text = Self::private_report_text(caption, bytes)?;
            return self.send_chunks(&chat, &text).await;
        }

        let mut handle = self.handle()?;
        let channel = {
            let chat = chat.clone();
            handle
                .with_connection(move |con| {
                    let state = con
                        .get_state()
                        .map_err(|e| AppError::adapter("teamspeak", e))?;
                    target_of(state, &chat)?;
                    channel_for(state, &chat)
                })
                .await
                .map_err(|e| AppError::adapter("teamspeak", e))??
        };
        let mut upload = timeout(
            UPLOAD_TIMEOUT,
            handle.upload_file(
                channel,
                format!("/{filename}"),
                None,
                bytes.len() as u64,
                true,
                false,
            ),
        )
        .await
        .map_err(|_| AppError::message("TeamSpeak file upload timed out"))?
        .map_err(|e| AppError::adapter("teamspeak", e))?;
        timeout(UPLOAD_WRITE_TIMEOUT, async {
            upload.stream.write_all(&bytes).await?;
            upload.stream.flush().await?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|_| AppError::message("TeamSpeak file upload timed out"))??;

        let path = format!("/{filename}");
        let line = if caption.is_empty() {
            path
        } else {
            format!("{caption}\n{path}")
        };
        self.send_chunks(&chat, &line).await
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TeamspeakAdapter {
    fn platform(&self) -> Platform {
        Platform::Teamspeak
    }

    fn key(&self) -> &ChannelKey {
        &self.key
    }

    fn profile(&self) -> &ChannelProfile {
        &self.profile
    }

    fn capabilities(&self) -> ChannelCapabilities {
        TEAMSPEAK_CAPABILITIES
    }

    async fn send(&self, msg: WireMessage) -> Result<SentMessage> {
        let WireMessage {
            chat,
            reference,
            payload,
        } = msg;
        let chat = self.chat_from_address(&chat)?;
        if reference.is_some() {
            return Err(AppError::message("TeamSpeak has no message references"));
        }
        let OutboundPayload::Text { content } = payload else {
            return Err(AppError::message("TeamSpeak chat carries text only"));
        };
        ensure_fits(&content)?;
        self.send_once(&chat, &content).await?;
        Ok(SentMessage {
            external_id: Uuid::now_v7().to_string(),
            sent_at: jiff::Timestamp::now(),
            reference_sent: false,
            platform_meta: None,
            attachments: Vec::new(),
        })
    }

    async fn delete_messages(
        &self,
        _chat_external_id: &str,
        ids: &[String],
    ) -> Result<DeleteReport> {
        Ok(DeleteReport {
            deleted: 0,
            failed: ids
                .iter()
                .map(|id| FailedDeletion {
                    external_id: id.clone(),
                    detail: "TeamSpeak clients cannot delete chat messages".to_owned(),
                })
                .collect(),
        })
    }

    async fn forget_cached(&self, _file_ids: &[String]) -> Result<u64> {
        Ok(0)
    }

    fn start_typing(&self, _chat_external_id: &str) -> Option<Box<dyn TypingGuard>> {
        None
    }

    async fn download_file(&self, _file_id: &str) -> Result<Vec<u8>> {
        Err(AppError::message("TeamSpeak chat carries no attachments"))
    }

    async fn file_url(&self, _file_id: &str) -> Result<String> {
        Err(AppError::message("TeamSpeak chat carries no attachments"))
    }
}

fn target_of(state: &data::Connection, chat: &ChatId) -> Result<MessageTarget> {
    match chat {
        ChatId::Server => Ok(MessageTarget::Server),
        ChatId::Room(id) => {
            let current = state
                .clients
                .get(&state.own_client)
                .map(|client| client.channel);
            if current != Some(*id) {
                return Err(AppError::message(
                    "TeamSpeak can only send to the channel the bot is in",
                ));
            }
            Ok(MessageTarget::Channel)
        }
        ChatId::Client(uid) => state
            .clients
            .values()
            .find(|client| {
                client
                    .uid
                    .as_ref()
                    .is_some_and(|candidate| candidate.to_string() == *uid)
            })
            .map(|client| MessageTarget::Client(client.id))
            .ok_or_else(|| AppError::message("the TeamSpeak client is not connected")),
    }
}

fn channel_for(state: &data::Connection, chat: &ChatId) -> Result<ChannelId> {
    match chat {
        ChatId::Room(id) => Ok(*id),
        ChatId::Server => state
            .clients
            .get(&state.own_client)
            .map(|client| client.channel)
            .filter(|id| id.0 != 0)
            .ok_or_else(|| AppError::message("the bot is not in a TeamSpeak channel")),
        ChatId::Client(_) => Err(AppError::message(
            "a private TeamSpeak report is not uploaded",
        )),
    }
}

fn ensure_fits(content: &str) -> Result<()> {
    if content.is_empty() {
        return Err(AppError::message("TeamSpeak chat text is empty"));
    }
    if content.len() > MAX_TEXT_BYTES {
        return Err(AppError::message(format!(
            "TeamSpeak chat text is {} UTF-8 bytes; the limit is {MAX_TEXT_BYTES}",
            content.len()
        )));
    }
    Ok(())
}

/// Every returned chunk is non-empty and no longer than `max` bytes.
fn chunks(text: &str, max: usize) -> Vec<&str> {
    assert!(max > 0, "a text chunk has a positive ceiling");
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.len() <= max {
            chunks.push(rest);
            break;
        }
        let mut end = 0;
        for (index, ch) in rest.char_indices() {
            if index + ch.len_utf8() > max {
                break;
            }
            end = index + ch.len_utf8();
        }
        if end == 0 {
            end = rest
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(rest.len());
        }
        let split = rest[..end]
            .rfind('\n')
            .map(|index| index + 1)
            .or_else(|| rest[..end].rfind(' ').map(|index| index + 1))
            .unwrap_or(end);
        chunks.push(&rest[..split]);
        rest = &rest[split..];
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vo::AccountId;

    fn adapter(allowed: Option<Vec<String>>) -> TeamspeakAdapter {
        let adapter = TeamspeakAdapter::new(
            ChannelKey::new("teamspeak", "main"),
            ChannelProfile {
                account_id: AccountId(1),
                username: String::new(),
                name: "hai".to_owned(),
            },
            allowed,
        );
        adapter.test_origin(ChatOrigin::new("server-uid"));
        adapter
    }

    fn address(external_id: &str) -> ChatAddress {
        ChatAddress::new(Some(ChatOrigin::new("server-uid")), external_id)
    }

    #[test]
    fn the_byte_ceiling_is_inclusive() {
        assert!(ensure_fits(&"a".repeat(1024)).is_ok());
        let err = ensure_fits(&"a".repeat(1025)).unwrap_err().to_string();
        assert!(err.contains("1024"), "{err}");
        assert!(ensure_fits("").is_err());
    }

    #[test]
    fn chunks_keep_every_byte_and_stay_inside_the_ceiling() {
        for text in [
            "a".repeat(2500),
            "line one\nline two\nline three\n".repeat(100),
            "汉".repeat(400),
        ] {
            let parts = chunks(&text, 1024);
            assert!(parts.iter().all(|part| !part.is_empty()));
            assert!(parts.iter().all(|part| part.len() <= 1024));
            assert_eq!(parts.concat(), text);
        }
    }

    #[test]
    fn chunks_prefer_a_newline_boundary() {
        let text = format!("{}\n{}", "a".repeat(20), "b".repeat(20));
        let parts = chunks(&text, 30);
        assert_eq!(
            parts,
            vec!["aaaaaaaaaaaaaaaaaaaa\n", "bbbbbbbbbbbbbbbbbbbb"]
        );
    }

    #[test]
    fn short_host_copy_is_one_chunk_and_empty_text_is_none() {
        assert_eq!(chunks("hello", 1024), vec!["hello"]);
        assert!(chunks("", 1024).is_empty());
    }

    #[test]
    fn capabilities_are_text_only() {
        let adapter = adapter(None);
        let capabilities = adapter.capabilities();
        assert_eq!(capabilities.forms.len(), 1);
        assert_eq!(capabilities.forms[0].kind, OutboundKind::Text);
        assert!(!capabilities.reference);
        assert!(!capabilities.quote);
        assert_eq!(capabilities.max_media_bytes, 0);
        assert_eq!(adapter.command_prefix(), "!");
        assert_eq!(adapter.platform(), Platform::Teamspeak);
    }

    #[test]
    fn deleting_a_message_reports_every_refusal() {
        let report = futures::executor::block_on(adapter(None).delete_messages(
            "teamspeak:main:channel:3",
            &["1".to_owned(), "2".to_owned()],
        ))
        .expect("deletion is best effort");
        assert_eq!(report.deleted, 0);
        assert_eq!(report.failed.len(), 2);
        assert!(
            report
                .failed
                .iter()
                .all(|failure| { failure.detail.contains("cannot delete chat messages") })
        );
    }

    #[test]
    fn a_private_chat_carries_the_server_origin() {
        let adapter = adapter(None);
        let chat = adapter
            .private_chat_external_id("abc=")
            .expect("the server identity is installed");
        assert_eq!(chat.origin, Some(ChatOrigin::new("server-uid")));
        assert_eq!(chat.external_id, "client:abc=");
    }

    #[test]
    fn admission_is_open_without_a_list_and_exact_with_one() {
        assert!(adapter(None).admits(&address("channel:3")));
        let narrowed = adapter(Some(vec!["channel:3".to_owned()]));
        assert!(narrowed.admits(&address("channel:3")));
        assert!(!narrowed.admits(&address("server")));
        assert!(!narrowed.admits(&address("client:abc=")));
        assert!(!adapter(Some(Vec::new())).admits(&address("channel:3")));
        assert!(!narrowed.admits(&ChatAddress::new(
            Some(ChatOrigin::new("other")),
            "channel:3"
        )));
    }

    #[test]
    fn a_private_report_is_text_not_an_upload() {
        let text =
            TeamspeakAdapter::private_report_text("report", b"body".to_vec()).expect("utf-8");
        assert_eq!(text, "report\n\nbody");
        assert!(TeamspeakAdapter::private_report_text("", vec![0xff]).is_err());
    }

    #[test]
    fn external_chat_ids_must_belong_to_this_server() {
        let adapter = adapter(None);
        assert_eq!(
            adapter
                .chat_from_address(&address("channel:3"))
                .expect("this server's chat")
                .to_string(),
            "channel:3"
        );
        assert!(
            adapter
                .chat_from_address(&ChatAddress::new(
                    Some(ChatOrigin::new("other")),
                    "channel:3"
                ))
                .is_err()
        );
    }
}
