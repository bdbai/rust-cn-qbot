use std::time::Duration;

use futures::{Sink, SinkExt, Stream, StreamExt};
use hyper::body::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{Mutex, MutexGuard};
use tokio::{sync::Notify, time::sleep};
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};
use tracing::{debug, error, info, warn};

use super::opcode::{OpCode, OpCodePayload};
use super::QBotEventMessageHandler;
use super::{deserialize_any_op, handle_dispatch_event, payload::*};
use crate::qbot::error::{QBotEventError, QBotEventResult};
use crate::qbot::QBotAuthorizer;

#[derive(Default)]
pub struct QBotWebSocketAuthGroup {
    mutex: Mutex<()>,
}

impl QBotWebSocketAuthGroup {
    pub fn new() -> Self {
        Self {
            mutex: Mutex::new(()),
        }
    }
}

struct QBotWebSocketHandshaked<'g> {
    heartbeat_interval: u64,
    _auth_guard: MutexGuard<'g, ()>,
}

struct QBotWebSocketSession<S> {
    ws: S,
    session_id: String,
    heartbeat_interval: u64,
    token: String,
    last_seq: i32,
}

fn deserialize_op<T: DeserializeOwned + OpCodePayload + std::fmt::Debug>(
    bytes: impl AsRef<[u8]>,
) -> QBotEventResult<QBotEventPayload<T>> {
    let res = serde_json::from_slice::<QBotEventPayload<T>>(bytes.as_ref());
    let payload = match res {
        Ok(payload) => {
            debug!("received event message: {:?}", payload);
            payload
        }
        Err(err) => {
            error!(
                "failed to parse event message {}: {:?}",
                String::from_utf8_lossy(bytes.as_ref()),
                err
            );
            return Err(err.into());
        }
    };
    if payload.opcode != T::OPCODE {
        error!(
            "unexpected opcode, expect {} got {}",
            T::OPCODE,
            payload.opcode
        );
        return Err(QBotEventError::ReturnCodeError(payload.opcode.0 as u32));
    }
    Ok(payload)
}

async fn receive_op<
    T: DeserializeOwned + OpCodePayload + std::fmt::Debug,
    S: Unpin + Stream<Item = Result<WsMessage, WsError>>,
>(
    ws: &mut S,
) -> QBotEventResult<QBotEventPayload<T>> {
    let msg = ws
        .next()
        .await
        .ok_or_else(|| QBotEventError::UnexpectedData("eof".into()))??;
    let msg = msg.into_data();
    deserialize_op::<T>(&*msg)
}

async fn send_op<T: Serialize + OpCodePayload, S: Unpin + Sink<WsMessage, Error = WsError>>(
    data: &T,
    ws: &mut S,
) -> QBotEventResult<()> {
    let payload = QBotEventPayload {
        opcode: T::OPCODE,
        data,
        seq: None,
        event_type: None,
    };
    let payload = serde_json::to_string(&payload)?;
    debug!("sending ws message: {}", payload);
    ws.send(WsMessage::Text(payload.into())).await?;
    Ok(())
}

impl<'g> QBotWebSocketHandshaked<'g> {
    async fn handshake<S: Unpin + Stream<Item = Result<WsMessage, WsError>>>(
        ws: &mut S,
        auth_group: &'g QBotWebSocketAuthGroup,
    ) -> QBotEventResult<Self> {
        let auth_guard = auth_group.mutex.lock().await;
        let QBotEventPayload {
            data: HelloPayload { heartbeat_interval },
            ..
        } = receive_op(ws).await?;

        Ok(Self {
            heartbeat_interval,
            _auth_guard: auth_guard,
        })
    }
    async fn authenticate<
        A: QBotAuthorizer,
        S: Unpin + Stream<Item = Result<WsMessage, WsError>> + Sink<WsMessage, Error = WsError>,
    >(
        &self,
        authorizer: A,
        mut ws: S,
    ) -> QBotEventResult<QBotWebSocketSession<S>> {
        // Workaround for error opcode 9
        sleep(Duration::from_millis(2000)).await;

        let mut token = authorizer
            .get_access_token()
            .await
            .map_err(QBotEventError::AccessTokenError)?;
        token.insert_str(0, "QQBot ");

        const PUBLIC_GUILD_MESSAGES: u64 = 1 << 30;
        const _DIRECT_MESSAGE: u64 = 1 << 12;
        let payload = IdentifyPayload {
            token: &token,
            intents: PUBLIC_GUILD_MESSAGES,
            shard: (0, 1),
            properties: Default::default(),
        };
        send_op(&payload, &mut ws).await?;

        let mut session = QBotWebSocketSession {
            ws,
            session_id: Default::default(),
            heartbeat_interval: self.heartbeat_interval,
            token,
            last_seq: -1,
        };
        let (res_metadata, res) = session.receive_any().await?;
        if res_metadata.opcode != OpCode::OP_DISPATCH {
            return Err(QBotEventError::ReturnCodeError(
                res_metadata.opcode.0 as u32,
            ));
        }
        if res_metadata.event_type.as_deref() != Some("READY") {
            return Err(QBotEventError::UnexpectedData(format!(
                "expect READY, got {}",
                res_metadata.event_type.unwrap_or_default()
            )));
        }
        let ready: QBotEventPayload<ReadyPayload> = serde_json::from_slice(&*res)?;
        session.session_id = ready.data.session_id;
        session.last_seq = res_metadata.seq.unwrap_or(-1);
        // FIXME: ws get disconnected every minute. Send heartbeat every 30s as a workaround.
        session.heartbeat_interval = 30;

        Ok(session)
    }
}

impl<S: Unpin + Stream<Item = Result<WsMessage, WsError>>> QBotWebSocketSession<S> {
    async fn receive_any(&mut self) -> QBotEventResult<(QBotEventAnyPayload, Bytes)> {
        let msg = self
            .ws
            .next()
            .await
            .ok_or_else(|| QBotEventError::UnexpectedData("eof".into()))??;
        let msg = msg.into_data();
        let payload = deserialize_any_op(&*msg)?;
        if let Some(seq) = payload.seq {
            self.last_seq = seq.max(self.last_seq);
        }
        Ok((payload, msg))
    }
}

impl<S: Unpin + Sink<WsMessage, Error = WsError>> QBotWebSocketSession<S> {
    async fn send_op<T: Serialize + OpCodePayload>(&mut self, data: &T) -> QBotEventResult<()> {
        send_op(data, &mut self.ws).await
    }
    async fn resume(&mut self, mut ws: S) -> Result<(), (S, QBotEventError)> {
        let payload = ResumePayload {
            token: &self.token,
            session_id: &self.session_id,
            seq: self.last_seq,
        };
        if let Err(e) = send_op(&payload, &mut ws).await {
            return Err((ws, e));
        }
        self.ws = ws;
        Ok(())
    }
}

pub async fn run_loop(
    ws_url: impl Into<String>,
    authorizer: impl QBotAuthorizer + Sync,
    mut handler: impl QBotEventMessageHandler,
    quit_signal: &Notify,
    auth_group: &QBotWebSocketAuthGroup,
) -> QBotEventResult<()> {
    let ws_url: String = ws_url.into();
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url.as_str()).await?;
    let mut session = QBotWebSocketHandshaked::handshake(&mut ws, auth_group)
        .await?
        .authenticate(&authorizer, ws)
        .await?;
    info!(
        "initial ws connected, url={}, handshake_interval={}",
        ws_url, session.heartbeat_interval
    );

    session.send_op(&HeartbeatPayload).await?;
    'outer: loop {
        let result = run_loop_inner(&mut session, &mut handler, quit_signal).await;
        let Err(mut err) = result else { break Ok(()) };
        'retry: loop {
            if err.is_ignoreable() {
                info!("ignoring ws error: {:?}", err);
                break 'retry;
            }
            error!("ws loop error {:?}", err);
            if !err.is_recoverable() {
                break 'outer Err(err);
            }
            if !err.is_invalid_session() {
                sleep(Duration::from_secs(5)).await;
            }
            info!("reconnecting ws");
            let (mut ws, _) = tokio_tungstenite::connect_async(ws_url.as_str()).await?;
            let handshake = QBotWebSocketHandshaked::handshake(&mut ws, auth_group).await?;
            if err.is_resumable() {
                info!("resuming ws session");
                match session.resume(ws).await {
                    Ok(()) => continue 'outer,
                    Err((_, resume_err)) => {
                        err = resume_err;
                        error!("failed to resume ws session: {:?}", err);
                        continue 'retry;
                    }
                }
            }
            info!("re-identifying ws session");
            session = handshake.authenticate(&authorizer, ws).await?;
            session.send_op(&HeartbeatPayload).await?;
            break 'retry;
        }
    }
}

async fn run_loop_inner<
    S: Unpin + Stream<Item = Result<WsMessage, WsError>> + Sink<WsMessage, Error = WsError>,
>(
    session: &mut QBotWebSocketSession<S>,
    handler: &mut impl QBotEventMessageHandler,
    quit_signal: &Notify,
) -> QBotEventResult<()> {
    'run_loop: loop {
        let (metadata, data) = tokio::select! {
            biased;
            _ = quit_signal.notified() =>{
                info!("closing ws session {}", session.session_id);
                session.ws.close().await?;
                break 'run_loop Ok(())
            },
            _ = sleep(Duration::from_secs(session.heartbeat_interval)) => {
                session.send_op(&HeartbeatPayload).await?;
                continue 'run_loop;
            },
            msg = session.receive_any() => msg,
        }?;
        let event_type = match metadata.opcode {
            OpCode::OP_DISPATCH => metadata.event_type.unwrap_or_default(),
            OpCode::OP_HEARTBEAT => {
                debug!("received heartbeat");
                session.send_op(&HeartbeatPayload).await?;
                continue 'run_loop;
            }
            OpCode::OP_RECONNECT => break Err(QBotEventError::ReturnCodeError(7)),
            OpCode::OP_INVALID_SESSION => break Err(QBotEventError::ReturnCodeError(9)),
            op @ OpCode::OP_HEARTBEAT_ACK | op @ OpCode::OP_HTTP_CALLBACK_ACK => {
                debug!("received ack, op={}", op);
                continue 'run_loop;
            }
            op => {
                warn!(
                    "unknown ws opcode {}: {}",
                    op,
                    String::from_utf8_lossy(&*data)
                );
                continue 'run_loop;
            }
        };
        handle_dispatch_event(&event_type, &data, handler)?;
    }
}
