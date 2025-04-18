use serde::de::DeserializeOwned;
use tracing::{debug, error};

mod opcode;
pub mod payload;
mod webhook;
pub mod ws;

use super::error::{QBotWsError, QBotWsResult};
use opcode::OpCodePayload;
use payload::*;

fn deserialize_op<T: DeserializeOwned + OpCodePayload + std::fmt::Debug>(
    bytes: impl AsRef<[u8]>,
) -> QBotWsResult<QBotWebSocketPayload<T>> {
    let res = serde_json::from_slice::<QBotWebSocketPayload<T>>(bytes.as_ref());
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
        return Err(QBotWsError::ReturnCodeError(payload.opcode.0 as u32));
    }
    Ok(payload)
}

pub trait QBotWsMessageHandler {
    fn handle_at_message(&mut self, _payload: AtMessageCreatePayload) {}
}
