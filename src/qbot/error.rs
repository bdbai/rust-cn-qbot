use std::io;

use serde::{de::DeserializeOwned, Deserialize};
use thiserror::Error;
use tokio_tungstenite::tungstenite::{error::ProtocolError, Error as WsError};
use tracing::info;

#[derive(Debug, Error)]
pub enum QBotApiError {
    #[error("error sending HTTP request: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("HTTP {status_code}: {code} {message} ({trace_id})")]
    ApiError {
        status_code: u16,
        code: u32,
        message: String,
        trace_id: String,
    },
}

pub type QBotApiResult<T> = std::result::Result<T, QBotApiError>;

#[derive(Debug, Clone, Deserialize)]
struct QBotApiErrorResponse {
    #[serde(default)]
    code: u32,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Error)]
pub enum QBotEventError {
    #[error("error connecting to WebSocket: {0}")]
    WsError(#[from] WsError),
    #[error("error occurred while serving webhook: {0}")]
    WebhookServeError(io::Error),
    #[error("Event server returned unexpected data: {0}")]
    UnexpectedData(String),
    #[error("error parsing JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("error getting access token: {0}")]
    AccessTokenError(QBotApiError),
    #[error("returned code: {0}")]
    ReturnCodeError(u32),
}

pub type QBotEventResult<T> = Result<T, QBotEventError>;

impl QBotEventError {
    pub fn is_ignoreable(&self) -> bool {
        matches!(self, QBotEventError::InvalidJson(_))
    }
    pub fn is_resumable(&self) -> bool {
        matches!(
            self,
            QBotEventError::ReturnCodeError(4008 | 4009)
                | QBotEventError::WsError(WsError::Protocol(
                    ProtocolError::ResetWithoutClosingHandshake
                ))
        )
    }
    pub fn is_reidentifiable(&self) -> bool {
        self.is_resumable()
            || matches!(
                self,
                QBotEventError::ReturnCodeError(7 | 4006..=4009 | 4900..=4913)
            )
    }
    pub fn is_invalid_session(&self) -> bool {
        matches!(self, QBotEventError::ReturnCodeError(9))
    }
    pub fn is_recoverable(&self) -> bool {
        match self {
            QBotEventError::ReturnCodeError(_) => {
                self.is_reidentifiable() || self.is_invalid_session()
            }
            QBotEventError::AccessTokenError(QBotApiError::ApiError { .. }) => false,
            _ => true,
        }
    }
}

pub(crate) trait QBotApiResultFromResponseExt {
    async fn to_qbot_result<T: DeserializeOwned>(self) -> QBotApiResult<T>;
}

impl QBotApiResultFromResponseExt for reqwest::Response {
    async fn to_qbot_result<T: DeserializeOwned>(self) -> QBotApiResult<T> {
        let status = self.status();
        let trace_id = self
            .headers()
            .get("x-tps-trace-id")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default()
            .into();
        info!(%trace_id, "Response Trace-Id");
        if status.is_success() {
            Ok(self.json().await?)
        } else {
            let error_response: QBotApiErrorResponse = self.json().await?;
            Err(QBotApiError::ApiError {
                status_code: status.as_u16(),
                code: error_response.code,
                message: error_response.message,
                trace_id,
            })
        }
    }
}
