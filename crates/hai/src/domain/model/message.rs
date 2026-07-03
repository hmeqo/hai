use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::{Account, Chat, Topic};
use crate::domain::vo::MessageId;

#[derive(Debug, Clone, toasty::Model)]
#[table = "message"]
pub struct Message {
    #[key]
    #[auto(increment)]
    pub id: i64,

    #[index]
    pub chat_id: i64,
    #[belongs_to(key = chat_id, references = id)]
    pub chat: toasty::Deferred<Chat>,

    pub account_id: Option<i64>,
    #[belongs_to(key = account_id, references = id)]
    pub account: toasty::Deferred<Option<Account>>,

    pub role: String,
    pub content: toasty::Json<serde_json::Value>,

    pub topic_id: Option<uuid::Uuid>,
    #[belongs_to(key = topic_id, references = id)]
    pub topic: toasty::Deferred<Option<Topic>>,

    pub interaction_status: String,
    pub reply_to_id: Option<i64>,
    #[belongs_to(key = reply_to_id, references = id)]
    pub reply_to: toasty::Deferred<Option<Message>>,

    pub external_id: Option<String>,
    pub meta: Option<toasty::Json<serde_json::Value>>,
    pub token_count: Option<i32>,
    pub sent_at: Option<jiff::Timestamp>,

    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Message {
    pub fn id_(&self) -> MessageId {
        MessageId(self.id)
    }

    pub fn status(&self) -> Option<MessageStatus> {
        self.interaction_status.parse().ok()
    }

    pub fn active_at(&self) -> Timestamp {
        self.sent_at.unwrap_or(self.created_at)
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Unread,
    Replied,
    Seen,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}
