//! Operator out-of-band settings: `conversation <CONVERSATION> <setting>` writes the preference
//! blob and the entries a chat command would. Opt-in with `HAI_TEST_DATABASE_URL`.

use std::collections::BTreeSet;

use hai::{
    agentcore::ModelKind,
    config::{
        AppConfig,
        conversation::{ContentPolicy, Language},
        schema::{InputModality, ModelConfig},
    },
    domain::{
        repo::Repos,
        service::{ConversationRecordService, SettingChange, SettingsChange, SettingsService},
        vo::{
            ChannelKey, ChatId, ConversationId, ConversationPreferences, EntryKind, HostAction,
            ModelAlias,
        },
    },
};
use sqlx::PgPool;

fn config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.conversation.content_policy = ContentPolicy::Unrestricted;
    cfg.model_roles.default.model = "main".into();
    cfg.models.insert(
        "main".into(),
        ModelConfig {
            model: "openai/gpt".into(),
            name: None,
            roles: BTreeSet::from([ModelKind::Chat]),
            input: BTreeSet::from([InputModality::Text]),
            max_tokens: None,
            context_window: None,
            temperature: None,
            thinking: None,
            pricing: None,
        },
    );
    cfg
}

fn changes() -> [SettingChange; 4] {
    [
        SettingChange::Language(SettingsChange::Set(Language::En)),
        SettingChange::Model(SettingsChange::Set(ModelAlias::new("main"))),
        SettingChange::ContentPolicy(SettingsChange::Set(ContentPolicy::Standard)),
        SettingChange::Persona(SettingsChange::Set("  test persona  ".into())),
    ]
}

fn resets() -> [SettingChange; 4] {
    [
        SettingChange::Language(SettingsChange::Reset),
        SettingChange::Model(SettingsChange::Reset),
        SettingChange::ContentPolicy(SettingsChange::Reset),
        SettingChange::Persona(SettingsChange::Reset),
    ]
}

#[tokio::test]
async fn set_writes_preferences_and_the_entries_a_chat_command_would() {
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
    let conversation: i64 =
        sqlx::query_scalar("INSERT INTO conversation (pending) VALUES ('[]'::jsonb) RETURNING id")
            .fetch_one(&pool)
            .await
            .expect("conversation");
    let chat: i64 = sqlx::query_scalar(
        "INSERT INTO chat (channel_id, conversation_id, alias, external_id, chat_type, name) \
         VALUES ($1, $2, 'chat-test', 'remote', 'group', 'Test room') RETURNING id",
    )
    .bind(channel)
    .bind(conversation)
    .fetch_one(&pool)
    .await
    .expect("chat");

    let repos = Repos::new(pool.clone());
    let service = SettingsService::new(repos.clone());
    let records = ConversationRecordService::new(repos.clone());
    let cfg = config();
    let conversation_id = ConversationId(conversation);
    let origin = ChatId(chat);

    for change in changes() {
        service
            .apply(&cfg, conversation_id, origin, change)
            .await
            .expect("apply setting");
    }
    let applied = repos
        .conversation_preference
        .get(conversation_id)
        .await
        .expect("preference row")
        .expect("stored preferences");
    let applied: ConversationPreferences =
        serde_json::from_value(applied.prefs).expect("preference blob");
    assert_eq!(applied.language, Some(Language::En));
    assert_eq!(applied.model, Some(ModelAlias::new("main")));
    assert_eq!(applied.content_policy, Some(ContentPolicy::Standard));
    assert_eq!(applied.persona.as_deref(), Some("test persona"));

    let entries = records
        .pending_entries(conversation_id)
        .await
        .expect("pending entries");
    let actions: Vec<HostAction> = entries
        .iter()
        .filter_map(|entry| entry.host_action().copied())
        .collect();
    assert_eq!(
        actions,
        vec![
            HostAction::LanguageChanged,
            HostAction::ModelChanged,
            HostAction::ContentPolicyChanged,
            HostAction::PersonaChanged,
        ]
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
            .count(),
        1,
        "a tighter level reopens the chapter once"
    );

    for change in changes() {
        service
            .apply(&cfg, conversation_id, origin, change)
            .await
            .expect("re-apply the same setting");
    }
    let entries = records
        .pending_entries(conversation_id)
        .await
        .expect("pending entries");
    assert_eq!(entries.len(), 5, "a no-op records nothing");

    for change in resets() {
        service
            .apply(&cfg, conversation_id, origin, change)
            .await
            .expect("reset setting");
    }
    let stored = repos
        .conversation_preference
        .get(conversation_id)
        .await
        .expect("preference row")
        .expect("stored preferences");
    let stored: ConversationPreferences =
        serde_json::from_value(stored.prefs).expect("preference blob");
    assert_eq!(stored, ConversationPreferences::default());
    let entries = records
        .pending_entries(conversation_id)
        .await
        .expect("pending entries");
    assert_eq!(entries.len(), 9, "four resets, and no tighter level");
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
            .count(),
        1,
        "a looser reset does not reopen the chapter"
    );
    for change in resets() {
        service
            .apply(&cfg, conversation_id, origin, change)
            .await
            .expect("reset absent preference");
    }
    assert_eq!(
        records
            .pending_entries(conversation_id)
            .await
            .expect("pending entries")
            .len(),
        9,
        "a reset with nothing stored records nothing"
    );

    let before = repos
        .conversation_preference
        .get(conversation_id)
        .await
        .expect("preference row")
        .map(|row| row.prefs);
    let error = service
        .apply(
            &cfg,
            conversation_id,
            origin,
            SettingChange::Model(SettingsChange::Set(ModelAlias::new("nope"))),
        )
        .await
        .expect_err("unknown model alias");
    assert!(error.to_string().contains("unknown model alias"), "{error}");
    let after = repos
        .conversation_preference
        .get(conversation_id)
        .await
        .expect("preference row")
        .map(|row| row.prefs);
    assert_eq!(before, after, "a rejected value writes nothing");

    sqlx::query("DELETE FROM chat WHERE id = $1")
        .bind(chat)
        .execute(&pool)
        .await
        .expect("delete chat");
    sqlx::query("DELETE FROM platform_channel WHERE id = $1")
        .bind(channel)
        .execute(&pool)
        .await
        .expect("delete channel");
    sqlx::query("DELETE FROM conversation WHERE id = $1")
        .bind(conversation)
        .execute(&pool)
        .await
        .expect("delete conversation");
}
