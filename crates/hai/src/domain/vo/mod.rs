/// An enum-typed column: the stored text decodes through the enum's own wire name, and a value
/// outside the enum fails the read with a message naming the enum and the value. Decoding here
/// rather than through `try_from` is what keeps that message readable — `sqlx` renders a
/// `TryFrom` error through a specialization that collapses to `unprintable error`.
macro_rules! enum_column {
    ($($name:ty),+ $(,)?) => {
        $(
            impl sqlx::Type<sqlx::Postgres> for $name {
                fn type_info() -> sqlx::postgres::PgTypeInfo {
                    <&str as sqlx::Type<sqlx::Postgres>>::type_info()
                }

                fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                    <&str as sqlx::Type<sqlx::Postgres>>::compatible(ty)
                }
            }

            impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
                fn decode(
                    value: sqlx::postgres::PgValueRef<'r>,
                ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
                    let raw = <&str as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
                    raw.parse()
                        .map_err(|_| format!("unknown {} value {raw:?}", stringify!($name)).into())
                }
            }
        )+
    };
}

pub(crate) use enum_column;

pub mod chat_address;
pub mod collection;
pub mod command;
pub mod content;
pub mod context_meta;
pub mod conversation_preferences;
pub mod conversation_snapshot;
pub mod entry;
pub mod event;
pub mod host_action;
pub mod id;
pub mod meta;
pub mod model_alias;
pub mod purge;
pub mod session;
pub mod step;
pub mod stretch;
pub mod subject;
pub mod time;
pub mod tool_call_result;

pub use chat_address::{ChatAddress, ChatOrigin};
pub use collection::{Collection, CollectionName};
pub use command::AgentCommand;
pub use content::{AttachmentKind, AttachmentMeta, AttachmentParser, ImageFormat, MediaCodec};
pub use context_meta::ContextMeta;
pub use conversation_preferences::{ConversationPreferences, PreferenceKey};
pub use conversation_snapshot::{ChapterWrite, ConversationSnapshot};
pub use entry::{ChatEntry, EntryAudience, EntryKind};
pub use event::{
    AgentEvent, AgentEventPayload, BatchComposition, EventKind, ModelRetryReason, StartedBy,
    StepOutput, TurnEndReason, WakeReasonKind,
};
pub use host_action::HostAction;
pub use id::{
    AccountId, AttachmentId, ChannelKey, ChatId, ConversationId, KnowledgeChunkId,
    KnowledgeDocumentId, MemoryId, MessageId, PerceptionId, PlatformChannelId, ScheduledTaskId,
    SessionId, StepNumber, TopicId, TurnNumber, attachment_id,
};
pub use meta::{
    LlmMessageMeta, MessageMeta, PlatformAccountMeta, PlatformMessageMeta, TeamspeakAccountMeta,
    TelegramAccountMeta, TelegramMessageMeta, TelegramQuote,
};
pub use model_alias::ModelAlias;
pub use purge::{PurgeCounts, PurgeSpec, PurgeSurface, PurgeSurfaces};
pub use session::{SessionKind, SessionRef};
pub use step::Step;
pub use stretch::{Stretch, TimeUnit, whole_units};
pub use subject::{Subject, SubjectKind};
pub use time::{RepeatInterval, TimeRange};
pub use tool_call_result::ToolCallResult;
