use crate::domain::{
    model::Platform,
    vo::{ChannelKey, ChatOrigin, ConversationId, PlatformChannelId},
};

/// One configured HAI channel: the local trust boundary every Chat and Account hangs off. A remote
/// origin is metadata on this row; it is never the key that authorizes a Chat or an Account.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PlatformChannel {
    pub id: PlatformChannelId,
    pub channel_key: ChannelKey,
    pub platform: Platform,
    pub origin: Option<ChatOrigin>,
    pub default_conversation_id: Option<ConversationId>,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}
