use std::time::Duration;

use futures::{Sink, SinkExt, Stream, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{sync::Notify, time::sleep};
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};
use tracing::{debug, error, info, warn};

mod opcode;
pub mod payload;

use super::error::{QBotWsError, QBotWsResult};
use super::QBotAuthorizer;
use opcode::{OpCode, OpCodePayload};
use payload::*;

struct QBotWebSocketHandshaked {
    heartbeat_interval: u64,
}

struct QBotWebSocketSession<S> {
    ws: S,
    session_id: String,
    heartbeat_interval: u64,
    token: String,
    last_seq: i32,
}

async fn receive_op<
    T: DeserializeOwned + OpCodePayload + std::fmt::Debug,
    S: Unpin + Stream<Item = Result<WsMessage, WsError>>,
>(
    ws: &mut S,
) -> QBotWsResult<QBotWebSocketPayload<T>> {
    let msg = ws
        .next()
        .await
        .ok_or_else(|| QBotWsError::UnexpectedData("eof".into()))??;
    let msg = msg.into_data();
    let payload = match serde_json::from_slice::<QBotWebSocketPayload<T>>(&msg) {
        Ok(payload) => {
            debug!("received ws message: {:?}", payload);
            payload
        }
        Err(err) => {
            error!(
                "failed to parse ws message {}: {:?}",
                String::from_utf8_lossy(&msg),
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
        return Err(QBotWsError::ReturnCodeError(payload.opcode.0 as u32));
    }
    Ok(payload)
}

async fn send_op<T: Serialize + OpCodePayload, S: Unpin + Sink<WsMessage, Error = WsError>>(
    data: &T,
    ws: &mut S,
) -> QBotWsResult<()> {
    let payload = QBotWebSocketPayload {
        opcode: T::OPCODE,
        data,
        seq: None,
        event_type: None,
    };
    let payload = serde_json::to_string(&payload)?;
    debug!("sending ws message: {}", payload);
    ws.send(WsMessage::Text(payload)).await?;
    Ok(())
}

impl QBotWebSocketHandshaked {
    async fn handshake<S: Unpin + Stream<Item = Result<WsMessage, WsError>>>(
        ws: &mut S,
    ) -> QBotWsResult<Self> {
        let QBotWebSocketPayload {
            data: HelloPayload { heartbeat_interval },
            ..
        } = receive_op(ws).await?;

        Ok(Self { heartbeat_interval })
    }
    async fn authenticate<
        A: QBotAuthorizer,
        S: Unpin + Stream<Item = Result<WsMessage, WsError>> + Sink<WsMessage, Error = WsError>,
    >(
        &self,
        authorizer: A,
        mut ws: S,
    ) -> QBotWsResult<QBotWebSocketSession<S>> {
        let mut token = authorizer
            .get_access_token()
            .await
            .map_err(QBotWsError::AccessTokenError)?;
        token.insert_str(0, "QQBot ");

        let payload = IdentifyPayload {
            token: &token,
            intents: 1 << 30, // PUBLIC_GUILD_MESSAGES,
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
            return Err(QBotWsError::ReturnCodeError(res_metadata.opcode.0 as u32));
        }
        if res_metadata.event_type.as_deref() != Some("READY") {
            return Err(QBotWsError::UnexpectedData(format!(
                "expect READY, got {}",
                res_metadata.event_type.unwrap_or_default()
            )));
        }
        let ready: QBotWebSocketPayload<ReadyPayload> = serde_json::from_slice(res.as_bytes())?;
        session.session_id = ready.data.session_id;
        session.last_seq = res_metadata.seq.unwrap_or(-1);
        // FIXME: ws get disconnected every minute. Send heartbeat every 30s as a workaround.
        session.heartbeat_interval = 30;
        Ok(session)
    }
}

impl<S: Unpin + Stream<Item = Result<WsMessage, WsError>>> QBotWebSocketSession<S> {
    async fn receive_any(&mut self) -> QBotWsResult<(QBotWebSocketAnyPayload, String)> {
        let msg = self
            .ws
            .next()
            .await
            .ok_or_else(|| QBotWsError::UnexpectedData("eof".into()))??;
        let msg = msg
            .into_text()
            .map_err(|_| QBotWsError::UnexpectedData("response with non-utf8".into()))?;
        let payload: QBotWebSocketAnyPayload = match serde_json::from_slice(msg.as_bytes()) {
            Ok(payload) => {
                debug!("received ws message: {}", msg);
                payload
            }
            Err(err) => {
                error!("failed to parse ws message {}: {:?}", msg, err);
                return Err(err.into());
            }
        };
        if let Some(seq) = payload.seq {
            self.last_seq = seq.max(self.last_seq);
        }
        Ok((payload, msg))
    }
}

impl<S: Unpin + Sink<WsMessage, Error = WsError>> QBotWebSocketSession<S> {
    async fn send_op<T: Serialize + OpCodePayload>(&mut self, data: &T) -> QBotWsResult<()> {
        send_op(data, &mut self.ws).await
    }
    async fn resume(&mut self, mut ws: S) -> Result<(), (S, QBotWsError)> {
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

pub trait QBotWsMessageHandler {
    fn handle_at_message(&mut self, _payload: AtMessageCreatePayload) {}
}

pub async fn run_loop(
    ws_url: impl Into<String>,
    authorizer: impl QBotAuthorizer + Sync,
    mut handler: impl QBotWsMessageHandler,
    quit_signal: &Notify,
) -> QBotWsResult<()> {
    let ws_url: String = ws_url.into();
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url.as_str()).await?;
    let mut handshake = QBotWebSocketHandshaked::handshake(&mut ws).await?;
    let mut session = handshake.authenticate(&authorizer, ws).await?;
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
            handshake = QBotWebSocketHandshaked::handshake(&mut ws).await?;
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
    handler: &mut impl QBotWsMessageHandler,
    quit_signal: &Notify,
) -> QBotWsResult<()> {
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
            OpCode::OP_RECONNECT => break Err(QBotWsError::ReturnCodeError(7)),
            OpCode::OP_INVALID_SESSION => break Err(QBotWsError::ReturnCodeError(9)),
            op @ OpCode::OP_HEARTBEAT_ACK | op @ OpCode::OP_HTTP_CALLBACK_ACK => {
                debug!("received ack, op={}", op);
                continue 'run_loop;
            }
            op => {
                warn!("unknown opcode {}: {}", op, data);
                continue 'run_loop;
            }
        };
        match &*event_type {
            "RESUMED" => {
                info!("resumed ws session");
            }
            "AT_MESSAGE_CREATE" => {
                let msg: QBotWebSocketPayload<AtMessageCreatePayload> =
                    serde_json::from_slice(data.as_bytes())?;
                handler.handle_at_message(msg.data);
            }
            "PUBLIC_MESSAGE_DELETE" => {
                info!("received ws event {}", event_type);
            }
            _ => {
                warn!("unhandled ws event {}", event_type);
            }
        }
    }
}
