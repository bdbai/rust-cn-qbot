use std::sync::Arc;

use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper::{body::Body, header::USER_AGENT, StatusCode};
use hyper::{body::Incoming as IncomingBody, Request, Response};
use serde::Serialize;
use tracing::{debug, error, info, warn};

use super::challenge::ChallengeGenerator;
use crate::qbot::event::opcode::OpCode;
use crate::qbot::event::payload::{WebhookChallengePayload, WebhookChallengeResponsePayload};
use crate::qbot::event::{
    deserialize_any_op, handle_dispatch_event, QBotEventMessageHandler, QBotEventPayload,
};
use crate::qbot::{QBotEventError, QBotEventResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ErrorResponse<'a> {
    error: &'a str,
}

#[derive(Debug, Clone)]
pub(super) struct QBotWebhookService<H> {
    pub handler: H,
    pub challenge_generator: Arc<ChallengeGenerator>,
}

impl<H: QBotEventMessageHandler> QBotWebhookService<H> {
    fn handle_request_body(&self, body: Bytes) -> QBotEventResult<Bytes> {
        let payload = deserialize_any_op(body.as_ref())?;
        match payload.opcode {
            OpCode::OP_DISPATCH => {
                debug!("Received dispatch event");
                let event_type = payload.event_type.unwrap_or_default();
                handle_dispatch_event(&event_type, &*body, &self.handler)?;
                Ok(b"{}"[..].into())
            }
            OpCode::OP_HTTP_CALLBACK_CHALLENGE => {
                info!("Received HTTP callback challenge");
                let payload: QBotEventPayload<WebhookChallengePayload> =
                    serde_json::from_slice(body.as_ref())?;
                let mut plain_material = payload.data.event_ts + &payload.data.plain_token;
                let signature = self
                    .challenge_generator
                    .calculate_challenge_response(&mut plain_material);
                let res = serde_json::to_vec(&WebhookChallengeResponsePayload {
                    plain_token: &*payload.data.plain_token,
                    signature: &*signature,
                })
                .unwrap();
                Ok(res.into())
            }
            op => {
                warn!(
                    "Unknown webhook opcode {:?}, raw: {}",
                    op,
                    String::from_utf8_lossy(&*body)
                );
                return Err(QBotEventError::UnexpectedData("Unknown opcode".into()));
            }
        }
    }

    pub async fn call(&self, req: Request<IncomingBody>) -> Result<Response<Bytes>, hyper::Error> {
        let ua = req
            .headers()
            .get(USER_AGENT)
            .map(|v| v.to_str().ok())
            .flatten()
            .unwrap_or("unknown");
        let app_id = req
            .headers()
            .get("X-Bot-Appid")
            .map(|v| v.to_str().ok())
            .flatten()
            .unwrap_or("unknown");
        debug!(%ua, %app_id, "Received request");
        let upper = req.body().size_hint().upper().unwrap_or(u64::MAX);
        if upper > 1024 * 64 {
            error!("Request body too large: {}", upper);
            return Ok(Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .body(
                    serde_json::to_vec(&ErrorResponse {
                        error: "Request body too large",
                    })
                    .unwrap()
                    .into(),
                )
                .unwrap());
        }
        let whole_body = req.collect().await?.to_bytes();
        let res = self.handle_request_body(whole_body);
        let mut res = match res {
            Ok(body) => Response::new(body),
            Err(QBotEventError::UnexpectedData(err)) => {
                error!("Webhook returning UnexpectedData error: {}", err);
                let mut response = Response::new(
                    serde_json::to_vec(&ErrorResponse { error: &err })
                        .unwrap()
                        .into(),
                );
                *response.status_mut() = StatusCode::BAD_REQUEST;
                response
            }
            Err(err) => {
                error!("Webhook returning error: {:?}", err);
                let mut response = Response::new(
                    serde_json::to_vec(&ErrorResponse {
                        error: "Internal server error",
                    })
                    .unwrap()
                    .into(),
                );
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                response
            }
        };
        res.headers_mut().insert(
            "content-type",
            "application/json; charset=utf-8".parse().unwrap(),
        );
        Ok(res)
    }
}
