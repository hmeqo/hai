//! One process's live ConversationAgents: spawned on demand, pruned when their task ends, and
//! drained on shutdown. The Conversation is the unit; its Chats may sit on different channels, and
//! the router is how one agent reaches each of them.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{RwLock, mpsc},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::AgentRuntime;
use crate::{
    agent::{
        channel::ChannelRouter,
        chat::{ConversationAgent, ConversationAgentHandle},
        events::Doorbell,
    },
    domain::vo::ConversationId,
};

#[derive(Clone)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<ConversationId, AgentEntry>>>,
    /// Every spawn takes the next value, so a task that ends can tell its own entry from the
    /// replacement that may already stand in its place.
    generations: Arc<AtomicU64>,
    router: ChannelRouter,
    runtime: AgentRuntime,
    shutdown: CancellationToken,
}

struct AgentEntry {
    handle: ConversationAgentHandle,
    lease: AgentLease,
    generation: u64,
}

/// The task and token an agent entry owns. Dropping the lease stops the task; the cloneable handle
/// is only a command proxy.
struct AgentLease {
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

impl AgentLease {
    fn new(shutdown: CancellationToken, task: JoinHandle<()>) -> Self {
        Self { shutdown, task }
    }

    fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    async fn join(mut self) {
        let _ = (&mut self.task).await;
    }
}

impl Drop for AgentLease {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.task.abort();
    }
}

impl AgentRegistry {
    pub fn new(router: ChannelRouter, runtime: AgentRuntime, shutdown: CancellationToken) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            generations: Arc::new(AtomicU64::new(0)),
            router,
            runtime,
            shutdown,
        }
    }

    pub async fn get_or_create(&self, conversation_id: ConversationId) -> ConversationAgentHandle {
        if let Some(entry) = self.agents.read().await.get(&conversation_id)
            && !entry.lease.is_finished()
        {
            return entry.handle.clone();
        }
        let mut agents = self.agents.write().await;
        agents.retain(|_, entry| !entry.lease.is_finished());
        if let Some(entry) = agents.get(&conversation_id)
            && !entry.lease.is_finished()
        {
            return entry.handle.clone();
        }
        let entry = self.spawn(conversation_id);
        let handle = entry.handle.clone();
        agents.insert(conversation_id, entry);
        handle
    }

    /// A live agent for the conversation, without spawning one.
    pub async fn get(&self, conversation_id: ConversationId) -> Option<ConversationAgentHandle> {
        let agents = self.agents.read().await;
        let entry = agents.get(&conversation_id)?;
        if entry.lease.is_finished() {
            return None;
        }
        Some(entry.handle.clone())
    }

    fn spawn(&self, conversation_id: ConversationId) -> AgentEntry {
        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        let doorbell = Doorbell::new();
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let (digest_tx, digest_rx) = mpsc::unbounded_channel();
        let runtime = self.runtime.clone();
        let router = self.router.clone();
        let agent_shutdown = self.shutdown.child_token();
        let task_shutdown = agent_shutdown.clone();
        let agents = self.agents.clone();
        let doorbell_for_turn = doorbell.clone();
        let task = tokio::spawn(async move {
            match ConversationAgent::new(runtime, conversation_id, router, task_shutdown, digest_rx)
                .await
            {
                Ok(mut agent) => agent.run(doorbell_for_turn, status_rx).await,
                Err(e) => tracing::error!(%conversation_id, "Failed to create agent: {e}"),
            }
            let mut agents = agents.write().await;
            if agents
                .get(&conversation_id)
                .is_some_and(|entry| entry.generation == generation)
            {
                agents.remove(&conversation_id);
            }
        });
        AgentEntry {
            handle: ConversationAgentHandle::new(conversation_id, doorbell, status_tx, digest_tx),
            lease: AgentLease::new(agent_shutdown, task),
            generation,
        }
    }

    pub async fn drain(&self, timeout: Duration) {
        let leases: Vec<AgentLease> = {
            let mut agents = self.agents.write().await;
            agents.drain().map(|(_, entry)| entry.lease).collect()
        };
        let deadline = Instant::now() + timeout;
        for lease in leases {
            if tokio::time::timeout_at(deadline, lease.join())
                .await
                .is_err()
            {
                tracing::warn!("agent drain timed out, remaining agents abandoned");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use tokio_util::sync::CancellationToken;

    use super::AgentLease;

    struct SetOnDrop(Arc<AtomicBool>);

    impl Drop for SetOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_dropped_lease_stops_its_task() {
        let shutdown = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let task = {
            let shutdown = shutdown.clone();
            let dropped = dropped.clone();
            tokio::spawn(async move {
                let _set = SetOnDrop(dropped);
                shutdown.cancelled().await;
            })
        };

        tokio::task::yield_now().await;
        drop(AgentLease::new(shutdown, task));

        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the task was stopped");
    }
}
