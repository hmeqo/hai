//! Operator cross-platform membership: attach/detach moves the Chat and its pending entries, while
//! message history stays in the source Conversation. Opt-in with `HAI_TEST_DATABASE_URL`.

use hai::domain::{
    repo::Repos,
    service::ConversationService,
    vo::{
        ChannelKey, ChapterWrite, ChatEntry, ChatId, ConversationId, EntryAudience, EntryKind,
        HostAction, MessageId, PurgeSurfaces, Stretch,
    },
};
use sqlx::PgPool;

#[tokio::test]
async fn attach_and_detach_move_pending_but_not_history() {
    let Ok(url) = std::env::var("HAI_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPool::connect(&url).await.expect("test database");
    let key = ChannelKey::new("test", &uuid::Uuid::new_v4().to_string());
    let channel: i64 = sqlx::query_scalar(
        "INSERT INTO platform_channel (channel_key, platform) \
         VALUES ($1, 'telegram') RETURNING id",
    )
    .bind(&key)
    .fetch_one(&pool)
    .await
    .expect("channel");
    let source: i64 =
        sqlx::query_scalar("INSERT INTO conversation (pending) VALUES ('[]'::jsonb) RETURNING id")
            .fetch_one(&pool)
            .await
            .expect("source conversation");
    let target: i64 =
        sqlx::query_scalar("INSERT INTO conversation (pending) VALUES ('[]'::jsonb) RETURNING id")
            .fetch_one(&pool)
            .await
            .expect("target conversation");
    let chat_id: i64 = sqlx::query_scalar(
        "INSERT INTO chat (channel_id, conversation_id, alias, external_id, chat_type) \
         VALUES ($1, $2, 'chat-test', 'remote', 'group') RETURNING id",
    )
    .bind(channel)
    .bind(source)
    .fetch_one(&pool)
    .await
    .expect("chat");
    let entry = ChatEntry::new(
        ChatId(chat_id),
        EntryAudience::Agent,
        EntryKind::MessageCreated {
            message_id: MessageId(1),
        },
        jiff::Timestamp::now(),
    );
    let action = ChatEntry::new(
        ChatId(chat_id),
        EntryAudience::Context,
        EntryKind::HostAction(HostAction::PersonaChanged),
        jiff::Timestamp::now(),
    );
    let policy = ChatEntry::new(
        ChatId(chat_id),
        EntryAudience::Agent,
        EntryKind::PolicyChanged,
        jiff::Timestamp::now(),
    );
    sqlx::query("UPDATE conversation SET pending = $2 WHERE id = $1")
        .bind(source)
        .bind(
            serde_json::to_value(vec![entry.clone(), action.clone(), policy.clone()])
                .expect("entry json"),
        )
        .execute(&pool)
        .await
        .expect("pending");
    sqlx::query(
        "INSERT INTO message (chat_id, conversation_id, role, text) \
         VALUES ($1, $2, 'user', 'history')",
    )
    .bind(chat_id)
    .bind(source)
    .execute(&pool)
    .await
    .expect("history");

    let service = ConversationService::new(Repos::new(pool.clone()));
    service
        .attach(ChatId(chat_id), ConversationId(target))
        .await
        .expect("attach");
    let moved: i64 = sqlx::query_scalar("SELECT conversation_id FROM chat WHERE id = $1")
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("moved chat");
    assert_eq!(moved, target);
    let summaries = service.list().await.expect("conversation list");
    let target_summary = summaries
        .iter()
        .find(|summary| summary.id == ConversationId(target))
        .expect("target conversation is listed");
    let listed = target_summary
        .chats
        .iter()
        .find(|chat| chat.id == ChatId(chat_id))
        .expect("moved chat is listed");
    assert_eq!(listed.alias, "chat-test");
    assert_eq!(listed.external_id, "remote");
    assert_eq!(listed.channel_key, key);
    let source_pending: serde_json::Value =
        sqlx::query_scalar("SELECT pending FROM conversation WHERE id = $1")
            .bind(source)
            .fetch_one(&pool)
            .await
            .expect("source pending");
    let stayed: Vec<ChatEntry> =
        serde_json::from_value(source_pending).expect("source pending entries");
    assert_eq!(
        stayed,
        vec![action, policy],
        "conversation-scoped entries stay in the source Conversation"
    );
    let target_pending: serde_json::Value =
        sqlx::query_scalar("SELECT pending FROM conversation WHERE id = $1")
            .bind(target)
            .fetch_one(&pool)
            .await
            .expect("target pending");
    let moved_entries: Vec<ChatEntry> =
        serde_json::from_value(target_pending).expect("target pending entries");
    assert_eq!(moved_entries, vec![entry.clone()]);
    let history: i64 = sqlx::query_scalar("SELECT conversation_id FROM message WHERE chat_id = $1")
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("history");
    assert_eq!(history, source, "message history must not move");

    let repos = Repos::new(pool.clone());
    let record = repos
        .conversation_record
        .find_by_id(ConversationId(target))
        .await
        .expect("target record")
        .expect("target conversation");
    repos
        .purge
        .wipe(
            ConversationId(target),
            PurgeSurfaces {
                agent: true,
                files: false,
                telegram: false,
                derived: false,
            },
            Stretch::All.resolve(jiff::Timestamp::now()),
            ChapterWrite {
                messages: record.messages,
                context_meta: record.context_meta,
            },
            &[],
        )
        .await
        .expect("purge target conversation");
    let history: i64 = sqlx::query_scalar("SELECT conversation_id FROM message WHERE chat_id = $1")
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("history survived purge");
    assert_eq!(
        history, source,
        "purge must not delete a detached Chat's history"
    );

    let fresh = service.detach(ChatId(chat_id)).await.expect("detach");
    assert_ne!(fresh, ConversationId(target));
    let detached: i64 = sqlx::query_scalar("SELECT conversation_id FROM chat WHERE id = $1")
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("detached chat");
    assert_eq!(detached, fresh.0);

    sqlx::query("DELETE FROM message WHERE chat_id = $1")
        .bind(chat_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chat WHERE id = $1")
        .bind(chat_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM platform_channel WHERE id = $1")
        .bind(channel)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM conversation WHERE id = ANY($1)")
        .bind([source, target, fresh.0])
        .execute(&pool)
        .await
        .unwrap();
}
