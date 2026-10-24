use std::time::Duration;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tsclientlib::{
    ChannelId, Connection, DisconnectOptions, Identity,
    sync::{SyncConnection, SyncConnectionHandle, SyncStreamItem},
};

use super::{config::TeamspeakConfig, inbound::arrival_from_message};
use crate::{
    agent::inbound::Arrival,
    domain::vo::ChatOrigin,
    error::{AppError, Result},
};

const JOIN_TIMEOUT: Duration = Duration::from_secs(10);
const DISCONNECT_DRAIN: Duration = Duration::from_secs(3);
const JOIN_POLL: Duration = Duration::from_millis(50);

pub(super) fn connect(cfg: &TeamspeakConfig, identity: Identity) -> Result<SyncConnection> {
    let mut options = Connection::build(cfg.address.clone())
        .name(cfg.name.clone())
        .identity(identity);
    if cfg.channel_id > 0 {
        options = options.channel_id(ChannelId(cfg.channel_id));
    } else if !cfg.channel.is_empty() {
        options = options.channel(cfg.channel.clone());
    }
    if !cfg.server_password.is_empty() {
        options = options.password(cfg.server_password.clone());
    }
    if !cfg.channel_password.is_empty() {
        options = options.channel_password(cfg.channel_password.clone());
    }
    let con = options
        .connect()
        .map_err(|e| AppError::adapter("teamspeak", e))?;
    Ok(con.into())
}

pub(super) async fn run_events(
    mut con: SyncConnection,
    config_name: String,
    arrivals: mpsc::UnboundedSender<Arrival>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            item = con.next() => match item {
                Some(Ok(SyncStreamItem::BookEvents(events))) => {
                    let batch: Vec<Arrival> = {
                        let state = match con.get_state() {
                            Ok(state) => state,
                            Err(error) => {
                                tracing::warn!(%error, "TeamSpeak state unavailable; dropping an event batch");
                                continue;
                            }
                        };
                        events
                            .into_iter()
                            .filter_map(|event| {
                                arrival_from_message(state, &config_name, event)
                            })
                            .collect()
                    };
                    for arrival in batch {
                        if arrivals.send(arrival).is_err() {
                            return;
                        }
                    }
                }
                Some(Ok(SyncStreamItem::DisconnectedTemporarily(reason))) => {
                    tracing::warn!(?reason, "TeamSpeak connection lost; reconnecting");
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    tracing::error!(%error, "TeamSpeak event stream failed");
                    return;
                }
                None => return,
            },
            _ = shutdown.cancelled() => {
                let _ = con.disconnect(DisconnectOptions::new());
                let _ = tokio::time::timeout(DISCONNECT_DRAIN, async {
                    while con.next().await.is_some() {}
                })
                .await;
                return;
            }
        }
    }
}

pub(super) async fn wait_until_joined(handle: &mut SyncConnectionHandle) -> Result<ChatOrigin> {
    let joined = tokio::time::timeout(JOIN_TIMEOUT, async {
        loop {
            let origin = handle
                .with_connection(|con| match con.get_state() {
                    Ok(state) => Ok(state
                        .clients
                        .contains_key(&state.own_client)
                        .then(|| ChatOrigin::new(state.server.public_key.get_uid()))),
                    Err(tsclientlib::Error::NotConnected) => Ok(None),
                    Err(error) => Err(AppError::adapter("teamspeak", error)),
                })
                .await
                .map_err(|e| AppError::adapter("teamspeak", e))??;
            if let Some(origin) = origin {
                return Ok(origin);
            }
            tokio::time::sleep(JOIN_POLL).await;
        }
    })
    .await;

    match joined {
        Ok(result) => result,
        Err(_) => Err(AppError::message(
            "TeamSpeak did not report the bot in a channel within 10s",
        )),
    }
}
