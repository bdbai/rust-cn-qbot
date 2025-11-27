use std::future::Future;
use std::sync::Arc;

#[cfg(test)]
use mock_instant::global::Instant;
#[cfg(not(test))]
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as TokioMutex;

use super::error::QBotApiResultFromResponseExt;
use super::json_u64::deserialize_json_u64;
use super::QBotApiResult;

pub trait QBotAuthorizer {
    fn get_access_token(&self) -> impl Future<Output = QBotApiResult<String>> + Send;
}

struct QBotAuthorizerImpl {
    base_url: String,
    app_id: String,
    client_secret: String,
}

pub struct QBotCachingAuthorizerImpl {
    inner: QBotAuthorizerImpl,
    last_response: TokioMutex<(Instant, GetAccessTokenResponse)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GetAccessTokenRequest<'a> {
    app_id: &'a str,
    client_secret: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct GetAccessTokenResponse {
    access_token: String,
    #[serde(deserialize_with = "deserialize_json_u64")]
    expires_in: u64,
}

impl QBotAuthorizerImpl {
    async fn get_access_token(&self) -> QBotApiResult<GetAccessTokenResponse> {
        let client = reqwest::Client::new();
        let res = client
            .post(format!("{}/app/getAppAccessToken", self.base_url))
            .json(&GetAccessTokenRequest {
                app_id: &self.app_id,
                client_secret: &self.client_secret,
            })
            .send()
            .await?;
        res.to_qbot_result().await
    }
}

impl QBotCachingAuthorizerImpl {
    pub async fn create_and_authorize(
        base_url: String,
        app_id: String,
        client_secret: String,
    ) -> QBotApiResult<Self> {
        let inner = QBotAuthorizerImpl {
            base_url,
            app_id,
            client_secret,
        };
        let now = Instant::now();
        let last_response = inner.get_access_token().await?;
        Ok(Self {
            inner,
            last_response: TokioMutex::new((now, last_response)),
        })
    }
}

impl QBotAuthorizer for QBotCachingAuthorizerImpl {
    async fn get_access_token(&self) -> QBotApiResult<String> {
        loop {
            let now = Instant::now();
            let mut last_response = self.last_response.lock().await;
            let (
                last_requested_at,
                GetAccessTokenResponse {
                    expires_in,
                    access_token,
                },
            ) = &*last_response;
            if now.duration_since(*last_requested_at).as_secs() < expires_in - 60 {
                return Ok(access_token.clone());
            }
            *last_response = (now, self.inner.get_access_token().await?);
        }
    }
}

impl<A: QBotAuthorizer> QBotAuthorizer for Arc<A>
where
    Arc<A>: Sync,
{
    async fn get_access_token(&self) -> QBotApiResult<String> {
        self.as_ref().get_access_token().await
    }
}

impl<A: QBotAuthorizer + Sync + ?Sized> QBotAuthorizer for &A {
    async fn get_access_token(&self) -> QBotApiResult<String> {
        (**self).get_access_token().await
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct MockAuthorizer(pub String);

#[cfg(test)]
impl QBotAuthorizer for MockAuthorizer {
    fn get_access_token(&self) -> impl Future<Output = QBotApiResult<String>> + Send {
        async move { Ok(self.0.clone()) }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mock_instant::global::MockClock;
    use mockito::Server;
    use serde_json::json;

    use crate::qbot::QBotApiError;

    use super::*;

    #[tokio::test]
    async fn test_create_and_authorize() {
        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("POST", "/app/getAppAccessToken")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::Json(json!({
                "appId": "givenAppId",
                "clientSecret": "givenClientSecret"
            })))
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "access_token": "givenAccessToken",
                    "expires_in": "7200"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let authorizer = QBotCachingAuthorizerImpl::create_and_authorize(
            mock_server.url(),
            "givenAppId".into(),
            "givenClientSecret".into(),
        )
        .await;
        let authorizer = authorizer.unwrap();
        let token = authorizer.get_access_token().await.unwrap();
        assert_eq!(token, "givenAccessToken");
        mock.assert_async().await;
    }
    #[tokio::test]
    async fn test_refresh_expired_access_token() {
        let mut mock_server = Server::new_async().await;
        let mock_init = mock_server
            .mock("POST", "/app/getAppAccessToken")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::Json(json!({
                "appId": "givenAppId",
                "clientSecret": "givenClientSecret"
            })))
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "access_token": "givenAccessToken",
                    "expires_in": "7200"
                })
                .to_string(),
            )
            .create_async()
            .await;
        let mock_refresh = mock_server
            .mock("POST", "/app/getAppAccessToken")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::Json(json!({
                "appId": "givenAppId",
                "clientSecret": "givenClientSecret"
            })))
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "access_token": "givenAccessToken2",
                    "expires_in": "7200"
                })
                .to_string(),
            )
            .create_async()
            .await;

        MockClock::set_time(Duration::from_secs(100));
        let authorizer = QBotCachingAuthorizerImpl::create_and_authorize(
            mock_server.url(),
            "givenAppId".into(),
            "givenClientSecret".into(),
        )
        .await;
        let authorizer = authorizer.unwrap();
        MockClock::advance(Duration::from_secs(7300));
        let token = authorizer.get_access_token().await.unwrap();
        assert_eq!(token, "givenAccessToken2");
        mock_init.assert_async().await;
        mock_refresh.assert_async().await;
    }
    #[tokio::test]
    async fn test_get_access_token_request_error() {
        let res = QBotCachingAuthorizerImpl::create_and_authorize(
            "chipichipi".into(),
            "givenAppId".into(),
            "givenClientSecret".into(),
        )
        .await;
        assert!(matches!(res, Err(QBotApiError::RequestError(_))));
    }
    #[tokio::test]
    async fn test_get_access_token_api_error() {
        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("POST", "/app/getAppAccessToken")
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_header("x-tps-trace-id", "givenTraceId")
            .with_body(
                json!({
                    "code": 114514,
                    "message": "givenMessage"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let res = QBotCachingAuthorizerImpl::create_and_authorize(
            mock_server.url(),
            "givenAppId".into(),
            "givenClientSecret".into(),
        )
        .await;
        match res {
            Ok(_) => panic!("unexpected result: Ok(_)"),
            Err(QBotApiError::ApiError {
                status_code: 400,
                code: 114514,
                message,
                trace_id,
            }) => {
                assert_eq!(message, "givenMessage");
                assert_eq!(trace_id, "givenTraceId");
            }
            Err(e) => panic!("unexpected result: {:?}", e),
        }
        mock.assert_async().await;
    }
}
