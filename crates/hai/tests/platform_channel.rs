//! The local channel boundary: accounts never share across channels, and an origin change is
//! refused. Opt-in: the default suite runs without PostgreSQL.

use hai::{
    config::conversation::ConversationScope,
    domain::{
        model::{ChatLocation, ChatType, Platform},
        repo::Repos,
        service::PlatformService,
        vo::{ChannelKey, ChatAddress, ChatOrigin},
    },
};
use sqlx::PgPool;

#[tokio::test]
async fn one_account_per_channel_and_an_origin_is_immutable() {
    let Ok(url) = std::env::var("HAI_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPool::connect(&url).await.expect("test database");
    let service = PlatformService::new(Repos::new(pool.clone()));
    let suffix = uuid::Uuid::new_v4();
    let first = ChannelKey::new("teamspeak", &format!("one-{suffix}"));
    let second = ChannelKey::new("teamspeak", &format!("two-{suffix}"));
    let address = ChatAddress::new(Some(ChatOrigin::new("server-a")), "chat");
    let scope = ConversationScope::Isolated;
    let location = |key| ChatLocation {
        channel_key: key,
        platform: Platform::Teamspeak,
        address: &address,
        scope,
    };

    let (_, first_account) = service
        .ensure_chat_and_account(
            location(&first),
            ChatType::Group,
            Some("one"),
            "person",
            None,
        )
        .await
        .expect("first channel");
    let (_, second_account) = service
        .ensure_chat_and_account(
            location(&second),
            ChatType::Group,
            Some("two"),
            "person",
            None,
        )
        .await
        .expect("second channel");
    assert_ne!(first_account.id, second_account.id);

    let changed_address = ChatAddress::new(Some(ChatOrigin::new("server-b")), "chat");
    let changed_location = ChatLocation {
        channel_key: &first,
        platform: Platform::Teamspeak,
        address: &changed_address,
        scope,
    };
    let error = service
        .ensure_platform_channel(
            &first,
            Platform::Teamspeak,
            Some(&ChatOrigin::new("server-b")),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("origin changed"), "{error}");
    assert!(
        service
            .get_or_create_chat(changed_location, ChatType::Group, None)
            .await
            .is_err()
    );

    let first_channel = service
        .ensure_platform_channel(&first, Platform::Teamspeak, None)
        .await
        .unwrap();
    let second_channel = service
        .ensure_platform_channel(&second, Platform::Teamspeak, None)
        .await
        .unwrap();
    let conversations: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT conversation_id FROM chat WHERE channel_id = ANY($1)")
            .bind([first_channel.id.0, second_channel.id.0])
            .fetch_all(&pool)
            .await
            .unwrap();
    for channel in [first_channel.id, second_channel.id] {
        sqlx::query("DELETE FROM account WHERE channel_id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat WHERE channel_id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM platform_channel WHERE id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM conversation WHERE id = ANY($1)")
        .bind(conversations)
        .execute(&pool)
        .await
        .unwrap();
}
