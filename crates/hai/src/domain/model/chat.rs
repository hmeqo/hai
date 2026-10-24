use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

use crate::{
    config::conversation::ConversationScope,
    domain::{
        model::Platform,
        vo::{
            ChannelKey, ChatAddress, ChatId, ChatOrigin, ConversationId, PlatformChannelId,
            enum_column,
        },
    },
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Chat {
    pub id: ChatId,

    pub channel_id: PlatformChannelId,
    /// Joined from the local channel; not stored on the chat row.
    pub platform: Platform,
    pub conversation_id: ConversationId,
    /// Stable inside its Conversation; the prompt uses it to name the message source.
    pub alias: String,
    pub external_id: String,
    pub chat_type: ChatType,
    pub name: Option<String>,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

/// The member Chat fields the prompt and dispatch need, without the address or timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRoster {
    pub id: ChatId,
    pub alias: String,
    pub platform: Platform,
    pub chat_type: ChatType,
    pub name: Option<String>,
}

impl Chat {
    pub fn roster(&self) -> ChatRoster {
        ChatRoster {
            id: self.id,
            alias: self.alias.clone(),
            platform: self.platform,
            chat_type: self.chat_type,
            name: self.name.clone(),
        }
    }
}

/// The local channel and remote address a chat is looked up or created by.
#[derive(Debug, Clone, Copy)]
pub struct ChatLocation<'a> {
    pub channel_key: &'a ChannelKey,
    pub platform: Platform,
    pub address: &'a ChatAddress,
    /// How a new Chat on this channel joins a Conversation.
    pub scope: ConversationScope,
}

/// The payload one chat row is written from.
#[derive(Debug, Clone)]
pub struct NewChat {
    pub channel_id: PlatformChannelId,
    pub external_id: String,
    pub chat_type: ChatType,
    pub name: Option<String>,
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    Private,
    Group,
    Channel,
}

enum_column!(ChatType);

impl ChatType {
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// The address a Chat row points at, assembled from its local channel's remote origin and its own
/// external id. The channel key is not part of the remote address.
pub fn chat_address(origin: Option<&ChatOrigin>, external_id: impl Into<String>) -> ChatAddress {
    ChatAddress::new(origin.cloned(), external_id)
}
