use std::str;
use std::sync::Arc;

use tracing::{debug, error, info, warn};

mod opcode;
pub mod payload;
pub mod webhook;
pub mod ws;

use super::error::QBotEventResult;
use payload::*;

fn deserialize_any_op(bytes: &[u8]) -> QBotEventResult<QBotEventAnyPayload> {
    let msg = bytes.as_ref();
    let payload: QBotEventAnyPayload = match serde_json::from_slice(msg) {
        Ok(payload) => {
            debug!("received event message: {}", String::from_utf8_lossy(msg));
            payload
        }
        Err(err) => {
            error!(
                "failed to parse event message {}: {:?}",
                String::from_utf8_lossy(msg),
                err
            );
            return Err(err.into());
        }
    };
    Ok(payload)
}

fn handle_dispatch_event(
    event_type: &str,
    data: &[u8],
    handler: &impl QBotEventMessageHandler,
) -> QBotEventResult<()> {
    match event_type {
        "RESUMED" => {
            info!("resumed ws session");
        }
        "AT_MESSAGE_CREATE" => {
            let msg: QBotEventPayload<AtMessageCreatePayload> = serde_json::from_slice(&*data)?;
            handler.handle_at_message(msg.data);
        }
        "DIRECT_MESSAGE_CREATE" => {
            let _msg: QBotEventPayload<DirectMessageCreatePayload> =
                serde_json::from_slice(&*data)?;
            // handler.handle_at_message(AtMessageCreatePayload {
            //     author: msg.data.author,
            //     channel_id: msg.data.channel_id,
            //     content: msg.data.content,
            //     guild_id: msg.data.guild_id,
            //     id: msg.data.id,
            //     member: msg.data.member,
            //     timestamp: msg.data.timestamp,
            //     seq: Default::default(),
            // })
        }
        "PUBLIC_MESSAGE_DELETE" => {
            info!("received event {}", event_type);
        }
        _ => {
            warn!("unhandled event {}", event_type);
        }
    }
    Ok(())
}

#[cfg_attr(test, mockall::automock)]
pub trait QBotEventMessageHandler {
    fn handle_at_message(&self, _payload: AtMessageCreatePayload) {}
}

impl<'a, H: QBotEventMessageHandler + ?Sized> QBotEventMessageHandler for &'a H {
    fn handle_at_message(&self, payload: AtMessageCreatePayload) {
        (**self).handle_at_message(payload)
    }
}

impl<H: QBotEventMessageHandler> QBotEventMessageHandler for Arc<H> {
    fn handle_at_message(&self, payload: AtMessageCreatePayload) {
        (**self).handle_at_message(payload)
    }
}
