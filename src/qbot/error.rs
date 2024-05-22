use serde::{de::DeserializeOwned, Deserialize};
use thiserror::Error;
use tokio_tungstenite::tungstenite::{error::ProtocolError, Error as WsError};

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
pub enum QBotWsError {
    #[error("error connecting to WebSocket: {0}")]
    WsError(#[from] WsError),
    #[error("WebSocket server returned unexpected data: {0}")]
    UnexpectedData(String),
    #[error("error parsing JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("error getting access token: {0}")]
    AccessTokenError(QBotApiError),
    #[error("returned code: {0}")]
    ReturnCodeError(u32),
}

pub type QBotWsResult<T> = Result<T, QBotWsError>;

impl QBotWsError {
    pub fn is_ignoreable(&self) -> bool {
        matches!(self, QBotWsError::InvalidJson(_))
    }
    pub fn is_resumable(&self) -> bool {
        matches!(
            self,
            QBotWsError::ReturnCodeError(4008 | 4009)
                | QBotWsError::WsError(WsError::Protocol(
                    ProtocolError::ResetWithoutClosingHandshake
                ))
        )
    }
    pub fn is_reidentifiable(&self) -> bool {
        self.is_resumable()
            || matches!(
                self,
                QBotWsError::ReturnCodeError(7 | 4006..=4009 | 4900..=4913)
            )
    }
    pub fn is_invalid_session(&self) -> bool {
        matches!(self, QBotWsError::ReturnCodeError(9))
    }
    pub fn is_recoverable(&self) -> bool {
        match self {
            QBotWsError::ReturnCodeError(_) => {
                self.is_reidentifiable() || self.is_invalid_session()
            }
            QBotWsError::AccessTokenError(QBotApiError::ApiError { .. }) => false,
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
        if status.is_success() {
            Ok(self.json().await?)
        } else {
            let trace_id = self
                .headers()
                .get("X-Trace-Id")
                .and_then(|h| h.to_str().ok())
                .unwrap_or_default()
                .into();
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
