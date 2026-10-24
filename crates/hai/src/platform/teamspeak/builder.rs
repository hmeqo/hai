use std::{path::Path, sync::Arc, time::Duration};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tsclientlib::{Identity, sync::SyncConnectionHandle};

use super::{
    adapter::TeamspeakAdapter,
    config::TeamspeakConfig,
    connection::{connect, run_events, wait_until_joined},
};
use crate::{
    agent::{
        inbound::{Arrival, Intake},
        process::{
            AgentRuntime, AppContext, startup_refeed::spawn_startup_refeed,
            task_watcher::spawn_task_watcher,
        },
    },
    config::schema::ChannelSection,
    domain::{model::Platform, service::Services, vo::ChannelKey},
    error::{AppError, Result},
    platform::{
        adapter::ChannelAdapter,
        host::{ChannelContext, ChannelRuntime, ChannelWiring},
        task::TaskGuard,
    },
};

/// The TeamSpeak side of one configured channel.
pub struct TeamspeakChannel;

impl TeamspeakChannel {
    pub async fn spawn(
        key: &str,
        section: &ChannelSection,
        ctx: &AppContext,
        runtime: &AgentRuntime,
        shutdown: CancellationToken,
        wiring: ChannelWiring,
    ) -> Result<ChannelRuntime> {
        let cfg = TeamspeakConfig::parse(key, &section.platform_settings())?;
        let scope = section.conversation_scope()?.unwrap_or_else(|| {
            super::super::host::platform_conversation_scope(Platform::Teamspeak)
        });
        let channel_key = ChannelKey::new(Platform::Teamspeak.as_str(), key);
        let identity = load_or_create_identity(&cfg.identity_path(key)?).await?;
        let account = ctx
            .services
            .platform
            .ensure_bot_account(Platform::Teamspeak, &channel_key)
            .await?;
        let profile = crate::agent::channel::ChannelProfile {
            account_id: account.id,
            username: String::new(),
            name: cfg.name.clone(),
        };
        let adapter = Arc::new(TeamspeakAdapter::new(
            channel_key,
            profile,
            cfg.allowed_chat_ids.clone(),
        ));
        let adapter_dyn: Arc<dyn ChannelAdapter> = adapter.clone();
        let context =
            ChannelRuntime::assemble(ctx, runtime, adapter_dyn, shutdown, scope, wiring).await;
        let supervisor = tokio::spawn(supervise(
            Arc::clone(&context),
            adapter,
            cfg,
            identity,
            ctx.services.clone(),
        ));
        Ok(ChannelRuntime::new(context, supervisor))
    }
}

const HEALTHY_GENERATION: Duration = Duration::from_secs(60);
const RESTART_BACKOFF_MAX: Duration = Duration::from_secs(60);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
enum GenerationExit {
    Cancelled,
    Died,
}

#[derive(Debug, Clone, Copy)]
enum Subsystem {
    Events,
    Dispatcher,
    Watcher,
}

impl Subsystem {
    fn label(self) -> &'static str {
        match self {
            Self::Events => "connection event task",
            Self::Dispatcher => "arrival dispatcher",
            Self::Watcher => "scheduled task watcher",
        }
    }
}

/// Reports a supervised task that ended without the shutdown token being cancelled: an unexpected
/// exit is a warning, a panic an error.
fn note_task_end(channel: &str, subsystem: Subsystem, result: Result<(), tokio::task::JoinError>) {
    match result {
        Ok(()) => tracing::warn!(channel, "{} exited unexpectedly", subsystem.label()),
        Err(e) if e.is_panic() => {
            tracing::error!(channel, "{} panicked", subsystem.label())
        }
        Err(_) => {}
    }
}

async fn dispatch_arrivals(mut arrivals: mpsc::UnboundedReceiver<Arrival>, intake: Arc<Intake>) {
    while let Some(arrival) = arrivals.recv().await {
        if let Err(e) = intake.accept(arrival).await {
            tracing::warn!(error = %e, "TeamSpeak arrival failed");
        }
    }
}

async fn run_generation(
    context: &ChannelContext,
    adapter: &Arc<TeamspeakAdapter>,
    cfg: &TeamspeakConfig,
    identity: &Identity,
    services: &Services,
) -> GenerationExit {
    let label = context.adapter.key().to_string();
    let mut start = context.start.clone();
    let _ = start.wait_for(|ready| *ready).await;
    let connection_shutdown = context.shutdown.child_token();
    let (arrivals, arrivals_rx) = mpsc::unbounded_channel();

    let con = match connect(cfg, identity.clone()) {
        Ok(con) => con,
        Err(e) => {
            tracing::warn!(channel = %label, error = %e, "TeamSpeak connection failed");
            return GenerationExit::Died;
        }
    };
    let mut handle: SyncConnectionHandle = con.get_handle();
    let session = adapter.open(handle.clone());
    let mut events = TaskGuard::spawn(run_events(
        con,
        cfg.name.clone(),
        arrivals,
        connection_shutdown.clone(),
    ));
    let origin = match wait_until_joined(&mut handle).await {
        Ok(origin) => origin,
        Err(e) => {
            tracing::warn!(channel = %label, error = %e, "TeamSpeak did not join its channel");
            connection_shutdown.cancel();
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *events.handle()).await;
            return GenerationExit::Died;
        }
    };
    if let Err(e) = session.joined(origin) {
        tracing::warn!(channel = %label, error = %e, "TeamSpeak session could not take its origin");
        connection_shutdown.cancel();
        let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *events.handle()).await;
        return GenerationExit::Died;
    }
    tracing::info!(channel = %label, "TeamSpeak client joined its channel");
    let mut dispatcher =
        TaskGuard::spawn(dispatch_arrivals(arrivals_rx, Arc::clone(&context.intake)));

    let mut watcher = TaskGuard::wrap(spawn_task_watcher(
        services.clone(),
        context.agents.clone(),
        context.adapter.key().clone(),
        context.shutdown.clone(),
    ));
    spawn_startup_refeed(
        services.clone(),
        context.agents.clone(),
        context.adapter.key().clone(),
    );

    let exit = tokio::select! {
        _ = context.shutdown.cancelled() => {
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *events.handle()).await;
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *dispatcher.handle()).await;
            GenerationExit::Cancelled
        }
        result = events.handle() => {
            if !context.shutdown.is_cancelled() {
                note_task_end(&label, Subsystem::Events, result);
            }
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *dispatcher.handle()).await;
            if context.shutdown.is_cancelled() {
                GenerationExit::Cancelled
            } else {
                GenerationExit::Died
            }
        }
        result = dispatcher.handle() => {
            if !context.shutdown.is_cancelled() {
                note_task_end(&label, Subsystem::Dispatcher, result);
            }
            connection_shutdown.cancel();
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *events.handle()).await;
            if context.shutdown.is_cancelled() {
                GenerationExit::Cancelled
            } else {
                GenerationExit::Died
            }
        }
        result = watcher.handle() => {
            if !context.shutdown.is_cancelled() {
                note_task_end(&label, Subsystem::Watcher, result);
            }
            connection_shutdown.cancel();
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, &mut *events.handle()).await;
            if context.shutdown.is_cancelled() {
                GenerationExit::Cancelled
            } else {
                GenerationExit::Died
            }
        }
    };
    if matches!(exit, GenerationExit::Cancelled) {
        tracing::info!(channel = %label, "supervisor stopped");
    }
    exit
}

async fn supervise(
    context: Arc<ChannelContext>,
    adapter: Arc<TeamspeakAdapter>,
    cfg: TeamspeakConfig,
    identity: Identity,
    services: Services,
) {
    let label = context.adapter.key().to_string();
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = tokio::time::Instant::now();
        let exit = run_generation(&context, &adapter, &cfg, &identity, &services).await;
        match exit {
            GenerationExit::Cancelled => return,
            GenerationExit::Died => {
                if started.elapsed() >= HEALTHY_GENERATION {
                    backoff = Duration::from_secs(1);
                }
                tracing::info!(
                    channel = %label,
                    backoff = backoff.as_secs(),
                    "restarting TeamSpeak connection"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(RESTART_BACKOFF_MAX);
            }
        }
    }
}

/// Loads the client identity once. A malformed file fails startup and is never overwritten; a
/// missing one is created and written so the next start keeps the same client identity.
async fn load_or_create_identity(path: &Path) -> Result<Identity> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| AppError::message(format!("identity {}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let identity = tokio::task::spawn_blocking(Identity::create)
                .await
                .map_err(|e| AppError::message(format!("identity {}: {e}", path.display())))?;
            write_identity(path, &identity)?;
            Ok(identity)
        }
        Err(e) => Err(AppError::message(format!(
            "identity {}: {e}",
            path.display()
        ))),
    }
}

fn write_identity(path: &Path, identity: &Identity) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(identity)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    Ok(())
}
