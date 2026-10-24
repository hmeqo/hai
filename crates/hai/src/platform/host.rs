//! The process's channels: one per `[channel.<name>]` section, started together and stopped on one
//! deadline.

use std::{str::FromStr, sync::Arc, time::Duration};

use strum::IntoEnumIterator;
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{
        channel::{Channel, ChannelAccess, ChannelRouter},
        inbound::{Intake, IntakeParts},
        process::{AgentRuntime, AppContext, registry::AgentRegistry},
    },
    config::conversation::ConversationScope,
    domain::model::Platform,
    error::{AppError, Result},
    platform::{
        adapter::ChannelAdapter, outbound::OutboundRecorder, teamspeak::TeamspeakChannel,
        telegram::TelegramChannel,
    },
};

/// Both platforms place new Chats in isolated Conversations by default.
pub(crate) fn platform_conversation_scope(platform: Platform) -> ConversationScope {
    match platform {
        Platform::Telegram | Platform::Teamspeak => ConversationScope::Isolated,
    }
}

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(45);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(40);
const _: () = assert!(SHUTDOWN_GRACE.as_secs() < SHUTDOWN_TIMEOUT.as_secs());

/// One channel's runtime, shared by the supervisor that owns it and the loops it starts.
pub(crate) struct ChannelContext {
    pub(crate) adapter: Arc<dyn ChannelAdapter>,
    /// The channel's one inbound path: what it has answered outlives a supervised generation.
    pub(crate) intake: Arc<Intake>,
    pub(crate) agents: AgentRegistry,
    pub(crate) shutdown: CancellationToken,
    /// Every channel is assembled before any intake starts.
    pub(crate) start: watch::Receiver<bool>,
}

/// The process-wide pieces a platform's channel needs to assemble and start.
#[derive(Clone)]
pub(crate) struct ChannelWiring {
    pub router: ChannelRouter,
    pub agents: AgentRegistry,
    pub start: watch::Receiver<bool>,
}

/// A channel the host holds: its runtime and the supervisor restarting its generations.
pub(crate) struct ChannelRuntime {
    pub(crate) context: Arc<ChannelContext>,
    pub(crate) supervisor: JoinHandle<()>,
}

impl ChannelRuntime {
    pub(crate) fn new(context: Arc<ChannelContext>, supervisor: JoinHandle<()>) -> Self {
        Self {
            context,
            supervisor,
        }
    }

    /// Everything every platform shares about running one channel: from an adapter on, they are all
    /// the same, so a platform supplies the adapter and gets back what a supervised run needs.
    pub(crate) async fn assemble(
        ctx: &AppContext,
        runtime: &AgentRuntime,
        adapter: Arc<dyn ChannelAdapter>,
        shutdown: CancellationToken,
        scope: ConversationScope,
        wiring: ChannelWiring,
    ) -> Arc<ChannelContext> {
        let channel: Arc<dyn Channel> = Arc::new(OutboundRecorder::new(
            Arc::clone(&adapter),
            ctx.services.clone(),
            runtime.model.clone(),
        ));
        wiring.router.register(channel).await;
        let choices = adapter.clone().choice_surface();
        let channel_key = adapter.key().clone();
        let access: Arc<dyn ChannelAccess> = adapter.clone();
        let intake = Arc::new(Intake::new(IntakeParts {
            channel_key,
            scope,
            access,
            choices,
            services: ctx.services.clone(),
            settings: ctx.settings.clone(),
            default_language: ctx.cfg.conversation.language,
            registry: wiring.agents.clone(),
        }));
        Arc::new(ChannelContext {
            adapter,
            intake,
            agents: wiring.agents,
            shutdown,
            start: wiring.start,
        })
    }
}

pub struct ChannelHost {
    channels: Vec<ChannelRuntime>,
    registry: AgentRegistry,
    agents_shutdown: CancellationToken,
}

impl ChannelHost {
    pub async fn spawn_all(ctx: &AppContext, runtime: &AgentRuntime) -> Result<Self> {
        let router = ChannelRouter::new();
        let agents_shutdown = CancellationToken::new();
        let registry = AgentRegistry::new(router.clone(), runtime.clone(), agents_shutdown.clone());
        let (start_tx, start_rx) = watch::channel(false);
        let mut host = ChannelHost {
            channels: Vec::new(),
            registry,
            agents_shutdown,
        };
        for (key, section) in &ctx.cfg.channel {
            // Without an explicit `type` the section's own name is the platform.
            let kind = section.kind.clone().unwrap_or_else(|| key.clone());
            let platform = resolve_platform(key, &kind)?;
            // A channel's stop belongs to the channel, not to the platform that starts it.
            let shutdown = CancellationToken::new();
            let channel = match platform {
                Platform::Telegram => {
                    TelegramChannel::spawn(
                        key,
                        section,
                        ctx,
                        runtime,
                        shutdown,
                        ChannelWiring {
                            router: router.clone(),
                            agents: host.registry.clone(),
                            start: start_rx.clone(),
                        },
                    )
                    .await?
                }
                Platform::Teamspeak => {
                    TeamspeakChannel::spawn(
                        key,
                        section,
                        ctx,
                        runtime,
                        shutdown,
                        ChannelWiring {
                            router: router.clone(),
                            agents: host.registry.clone(),
                            start: start_rx.clone(),
                        },
                    )
                    .await?
                }
            };
            host.channels.push(channel);
        }
        let _ = start_tx.send(true);
        Ok(host)
    }

    pub async fn shutdown(mut self) {
        for channel in &self.channels {
            channel.context.shutdown.cancel();
        }
        self.agents_shutdown.cancel();

        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        for channel in std::mem::take(&mut self.channels) {
            let ChannelRuntime {
                context,
                supervisor,
            } = channel;
            if tokio::time::timeout_at(deadline, supervisor).await.is_err() {
                tracing::warn!(channel = %context.adapter.key(), "shutdown timeout, abandoning");
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if tokio::time::timeout_at(deadline, self.registry.drain(SHUTDOWN_GRACE.min(remaining)))
            .await
            .is_err()
        {
            tracing::warn!("agent drain timeout, abandoning");
        }
    }
}

/// The platform one `[channel.<name>]` section names; an unknown name fails startup with the names
/// this build supports.
fn resolve_platform(key: &str, kind: &str) -> Result<Platform> {
    Platform::from_str(kind).map_err(|e| {
        AppError::message(format!(
            "channel '{key}': unknown platform '{kind}', supported: {}: {e}",
            Platform::iter()
                .map(|platform| platform.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

impl Drop for ChannelHost {
    fn drop(&mut self) {
        self.agents_shutdown.cancel();
        for channel in &mut self.channels {
            channel.context.shutdown.cancel();
            channel.supervisor.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sections_name_or_its_type_names_the_platform() {
        assert_eq!(
            resolve_platform("telegram", "telegram").expect("a known platform"),
            Platform::Telegram
        );
        assert_eq!(
            resolve_platform("main", "telegram").expect("a known platform"),
            Platform::Telegram
        );
        assert_eq!(
            resolve_platform("ts", "teamspeak").expect("a known platform"),
            Platform::Teamspeak
        );
    }

    #[test]
    fn every_platform_defaults_to_isolated() {
        assert_eq!(
            platform_conversation_scope(Platform::Teamspeak),
            ConversationScope::Isolated
        );
        assert_eq!(
            platform_conversation_scope(Platform::Telegram),
            ConversationScope::Isolated
        );
    }

    #[test]
    fn an_unknown_platform_names_the_section_and_what_is_supported() {
        let err = resolve_platform("main", "qq").unwrap_err().to_string();
        assert!(err.contains("channel 'main'"), "{err}");
        assert!(err.contains("unknown platform 'qq'"), "{err}");
        assert!(err.contains("telegram"), "{err}");
        assert!(err.contains("teamspeak"), "{err}");
    }
}
