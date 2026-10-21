//! One channel as the agent sees it: `Channel` is the outbound side (send, files, typing, and what
//! that channel declares); `ChannelAccess` is what the inbound tail and the commands it hands a page
//! to are given — admission, host copy, a page of buttons and its press, and a file. The platform
//! layer implements both — an agent module holds a `Channel` and never a platform type. Acting on a
//! chat goes through `ChannelHandle`, which binds the chat the agent belongs to.

use std::{fmt::Debug, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
    agent::web::WebUrl,
    domain::vo::{AccountId, ChannelKey, ChatId, MessageId, TopicId},
    error::Result,
};

/// The channel's own identity, as the agent context renders it.
#[derive(Debug, Clone)]
pub struct ChannelProfile {
    pub account_id: AccountId,
    pub username: String,
    pub name: String,
}

/// What one message points at inside the chat it goes to.
#[derive(Debug)]
pub struct ReplyTo {
    pub topic_id: Option<TopicId>,
    pub reference: Option<MessageId>,
    /// A part of that message to point at, copied verbatim from it.
    pub quote: Option<String>,
}

/// The bytes of one outbound message's media, or where the platform should fetch them.
#[derive(Debug, Clone)]
pub enum OutboundSource {
    Bytes(Vec<u8>),
    Url(WebUrl),
}

#[derive(Debug, Clone)]
pub enum OutboundPayload {
    Text {
        content: String,
    },
    Photo {
        source: OutboundSource,
        content: Option<String>,
    },
    Voice {
        bytes: Vec<u8>,
        content: Option<String>,
        /// What the speech model was asked for; kept on the attachment row.
        prompt: String,
    },
    Document {
        source: OutboundSource,
        filename: String,
        content: Option<String>,
    },
}

/// One button of a choice the host puts out: the label the client shows and the token the server
/// reads back. The token is opaque to the platform: it only has to survive the round trip.
#[derive(Debug, Clone)]
pub struct ChoiceOption {
    pub label: String,
    pub token: String,
}

/// What deleting a batch of platform messages came to. `failed` carries the platform's own words per
/// message; the host reports how many there were and leaves the reasons to the log.
#[derive(Debug, Clone, Default)]
pub struct DeleteReport {
    pub deleted: u64,
    pub failed: Vec<FailedDeletion>,
}

#[derive(Debug, Clone)]
pub struct FailedDeletion {
    pub external_id: String,
    pub detail: String,
}

/// One outgoing message with its chat resolved: what the platform layer is handed to send.
#[derive(Debug)]
pub struct OutboundMessage {
    pub chat_id: ChatId,
    pub reply: ReplyTo,
    pub payload: OutboundPayload,
}

/// One chat on the channel the agent runs on. The chat is bound here, so every verb that acts on it
/// already knows which one, and no caller can aim a send or an indicator at another chat.
#[derive(Debug, Clone)]
pub struct ChannelHandle {
    channel: Arc<dyn Channel>,
    chat_id: ChatId,
}

impl ChannelHandle {
    pub fn new(channel: Arc<dyn Channel>, chat_id: ChatId) -> Self {
        Self { channel, chat_id }
    }

    /// The chat this handle acts on.
    pub fn chat_id(&self) -> ChatId {
        self.chat_id
    }

    pub fn key(&self) -> &ChannelKey {
        self.channel.key()
    }

    pub fn profile(&self) -> &ChannelProfile {
        self.channel.profile()
    }

    pub fn capabilities(&self) -> ChannelCapabilities {
        self.channel.capabilities()
    }

    /// Sends something the agent produced into this handle's chat.
    pub async fn send(&self, reply: ReplyTo, payload: OutboundPayload) -> Result<()> {
        self.channel
            .send(OutboundMessage {
                chat_id: self.chat_id,
                reply,
                payload,
            })
            .await
    }

    /// Delivers copy the host composed itself (a receipt, a notice) into this handle's chat; nothing
    /// of it is stored, because the conversation the model reads is the one people had with it.
    pub async fn notice(&self, text: &str) -> Result<()> {
        self.channel.notice(self.chat_id, text).await
    }

    /// Deletes platform messages this handle's chat holds; what the platform refused comes back
    /// named rather than as a failure.
    pub async fn delete_messages(&self, ids: &[String]) -> Result<DeleteReport> {
        self.channel.delete_messages(self.chat_id, ids).await
    }

    /// Drops the bytes cached locally for these platform files, and says how many it found.
    pub async fn forget_cached(&self, file_ids: &[String]) -> Result<u64> {
        self.channel.forget_cached(file_ids).await
    }

    /// Starts the platform's typing indicator for this handle's chat and hands back the guard
    /// holding it alive; the platform answers `None` when it shows no indicator.
    #[must_use = "dropping the guard ends the indicator it started"]
    pub async fn start_typing(&self) -> Option<Box<dyn TypingGuard>> {
        self.channel.start_typing(self.chat_id).await
    }

    /// Downloads a platform file's bytes, for attachment analysis.
    pub async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        self.channel.download_file(file_id).await
    }

    /// A platform file's URL, for multimodal analysis.
    pub async fn file_url(&self, file_id: &str) -> Result<String> {
        self.channel.file_url(file_id).await
    }
}

/// One way a message can go out; a platform serves a subset of these.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OutboundKind {
    Text,
    Voice,
    File,
}

/// One argument a form takes.
#[derive(Debug, Clone, Copy)]
pub struct OutboundField {
    pub name: &'static str,
    pub description: &'static str,
}

/// What a form needs this build to have configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCapability {
    None,
    Speech,
}

/// One way a platform puts something in a chat: the kind it sends, the arguments that form takes,
/// and what the build must have for the form to exist.
#[derive(Debug, Clone, Copy)]
pub struct OutboundForm {
    pub kind: OutboundKind,
    pub fields: &'static [OutboundField],
    pub requires: BuildCapability,
}

/// What one platform can put in a chat: the forms it serves and the reply arguments it accepts; the
/// send tool's surface is pruned from it, so a platform is never asked for what it cannot do.
#[derive(Debug, Clone, Copy)]
pub struct ChannelCapabilities {
    pub forms: &'static [OutboundForm],
    /// A message can point at another one.
    pub reference: bool,
    /// A message can quote a part of the one it points at.
    pub quote: bool,
    /// The platform can put out a page of buttons and read a press back.
    pub choices: bool,
    /// The largest single upload this platform takes; a bigger local file is refused before it is read.
    pub max_media_bytes: u64,
}

/// A live "typing" indicator: the value's lifetime *is* the indicator's — dropping it ends what the
/// platform started, and how the platform keeps it alive between those two points is that platform's
/// own business.
pub trait TypingGuard: Send + 'static {}

/// What the agent needs from the channel it runs on: its identity, what it offers, and the few
/// things it can do. A platform implements the adapter (`platform::adapter::ChannelAdapter`); the
/// platform layer answers this trait around it, so the agent never reaches a vendor API or the
/// database. One value is one configured channel, shared by every chat on it: the agent acts on a
/// chat through `ChannelHandle`, and a verb that takes no chat acts on the channel itself.
#[async_trait::async_trait]
pub trait Channel: Debug + Send + Sync + 'static {
    /// The key this channel instance is stored under, spelled once where it is built.
    fn key(&self) -> &ChannelKey;
    fn profile(&self) -> &ChannelProfile;
    fn capabilities(&self) -> ChannelCapabilities;
    /// Sends something the agent produced and records what left.
    async fn send(&self, msg: OutboundMessage) -> Result<()>;
    /// Delivers copy the host composed itself into one of this channel's chats. Nothing is recorded:
    /// host copy is the frame around the conversation, not a turn in it.
    async fn notice(&self, chat_id: ChatId, text: &str) -> Result<()>;
    /// Deletes platform messages in one of this channel's chats. Best effort by contract: what the
    /// platform refuses comes back named, and never fails the call.
    async fn delete_messages(&self, chat_id: ChatId, ids: &[String]) -> Result<DeleteReport>;
    /// Drops the local bytes cached for these platform files, and says how many it found.
    async fn forget_cached(&self, file_ids: &[String]) -> Result<u64>;
    /// Downloads a platform file's bytes, for attachment analysis.
    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>>;
    /// A platform file's URL, for multimodal analysis.
    async fn file_url(&self, file_id: &str) -> Result<String>;
    /// Starts the platform's typing indicator for this chat and hands back the guard holding it
    /// alive. `None` when the platform shows no such indicator, or when it could not be started —
    /// the indicator is decoration, so a failure is logged and never reaches the caller.
    #[must_use = "dropping the guard ends the indicator it started"]
    async fn start_typing(&self, chat_id: ChatId) -> Option<Box<dyn TypingGuard>>;
}

/// What an inbound path needs from the channel's platform: whether a chat may talk here, how to
/// deliver copy the host composed, and how to put a question to a person and read the answer. The
/// tail reads it, and so do the commands it hands a page to (`purge`, `digest`).
#[async_trait::async_trait]
pub trait ChannelAccess: Debug + Send + Sync + 'static {
    /// Whether this chat may use the channel; the allowlist spelling is the platform's own.
    fn admits(&self, chat_external_id: &str) -> bool;
    /// How this platform spells a command: `/` where a slash line is the convention, `!` where it is
    /// not. Host copy that names a form prints it with this prefix.
    fn command_prefix(&self) -> &'static str;
    /// Delivers host copy (a greeting, an unauthorized notice); never stored.
    async fn notice(&self, chat_external_id: &str, text: &str) -> Result<()>;
    /// Puts copy out with a row of buttons under it. Host copy like any other: delivered, never
    /// stored. What a token means belongs to the caller; how the buttons are laid out is the
    /// platform's.
    async fn ask(
        &self,
        chat_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()>;
    /// Rewrites one of the host's own button messages; no options takes the buttons away.
    async fn edit_choice(
        &self,
        chat_external_id: &str,
        message_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()>;
    /// Answers one button press, which is what stops the client from waiting.
    async fn answer_callback(&self, query_id: &str, answer: CallbackAnswer<'_>) -> Result<()>;
    /// Takes one of the host's own messages away, after the page it carried has done its job.
    /// Host copy, so a refusal is cosmetic.
    async fn delete_notice(&self, chat_external_id: &str, message_external_id: &str) -> Result<()>;
    /// Delivers a file the host puts out itself (a digest's report), rather than a message the agent
    /// sent: it bypasses the conversation and is never recorded, like any other host copy. `caption`
    /// goes out with the file.
    async fn notice_document(
        &self,
        chat_external_id: &str,
        filename: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()>;
}

/// What answering a press says: nothing at all (the client stops waiting and shows nothing), a toast
/// that fades on its own, or a popup that waits to be dismissed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackAnswer<'a> {
    Quiet,
    Toast(&'a str),
    Alert(&'a str),
}
