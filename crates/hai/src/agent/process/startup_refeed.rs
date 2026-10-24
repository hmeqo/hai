use crate::{
    agent::process::registry::AgentRegistry,
    domain::{service::Services, vo::ChannelKey},
    error::Result,
};

/// Rings every Conversation under one channel whose accumulation is still unconsumed, once, at
/// generation start. It stops on its own, so it hands back nothing to hold.
pub fn spawn_startup_refeed(services: Services, registry: AgentRegistry, channel: ChannelKey) {
    tokio::spawn(async move {
        if let Err(e) = refeed(&services, &registry, &channel).await {
            tracing::error!(channel = %channel, "startup re-feed failed: {e}");
        }
    });
}

async fn refeed(services: &Services, registry: &AgentRegistry, channel: &ChannelKey) -> Result<()> {
    let conversations = services
        .conversation
        .conversations_with_pending(channel)
        .await?;
    if conversations.is_empty() {
        return Ok(());
    }

    tracing::info!(
        conversations = conversations.len(),
        channel = %channel,
        "re-feeding unconsumed changes"
    );
    for conversation_id in conversations {
        registry.get_or_create(conversation_id).await.wake();
    }
    Ok(())
}
