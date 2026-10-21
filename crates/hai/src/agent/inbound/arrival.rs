//! What a platform hands the inbound tail: one update read into platform-neutral fields.

use crate::domain::{
    model::{ChatType, Platform},
    repo::ChangeKind,
    vo::{AttachmentKind, AttachmentMeta, EntryAudience},
};

/// One message a platform read off its wire, in the shape the store and the agent speak.
pub(crate) struct Arrival {
    pub platform: Platform,
    pub chat: ArrivalChat,
    /// Absent for a message the platform reports without a sender (a channel post); the dispatcher
    /// drops those before they reach here.
    pub sender: Option<ArrivalSender>,
    pub text: String,
    /// The files the platform reported, in the platform's own terms.
    pub attachments: Vec<ArrivalAttachment>,
    pub external_id: String,
    pub reply_external_id: Option<String>,
    pub change: ChangeKind,
    pub sent_at: Option<jiff::Timestamp>,
    pub audience: EntryAudience,
    /// The platform's own message extras, stored as the row's meta.
    pub meta: serde_json::Value,
    pub text_kind: ArrivalText,
}

pub(crate) struct ArrivalChat {
    pub external_id: String,
    pub kind: ChatType,
    pub title: Option<String>,
}

pub(crate) struct ArrivalSender {
    pub external_id: String,
    pub meta: Option<serde_json::Value>,
}

/// One file a platform message carried, in the platform's own terms: the platform's two handles, the
/// kind and whatever it carries — but no row identity, which the tail derives.
pub(crate) struct ArrivalAttachment {
    pub kind: AttachmentKind,
    pub file_unique_id: Option<String>,
    pub file_id: String,
    pub caption: Option<String>,
    pub meta: AttachmentMeta,
}

/// What the platform made of the message's text.
pub(crate) enum ArrivalText {
    /// Ordinary prose: the platform found no command spelling in it.
    Ordinary,
    /// The platform's own greeting convention (Telegram's `/start`), answered and never stored.
    Greeting,
    /// A command of ours, with the payload the platform sliced off.
    Command { name: String, payload: String },
    /// One of our own buttons was pressed: an exchange with the host, not a message anybody wrote.
    Choice {
        /// The opaque token the host put on the button.
        token: String,
        /// What the platform needs to stop the client waiting for an answer.
        query_id: String,
        /// The message the buttons hang on, so the host can rewrite it in place.
        message_external_id: String,
    },
}
