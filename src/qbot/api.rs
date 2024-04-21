use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{error::QBotApiResultFromResponseExt, QBotApiResult, QBotAuthorizer};

pub trait QBotApiClient {
    fn reply_text_to_channel_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> impl Future<Output = QBotApiResult<()>> + Send;
}

pub struct QBotApiClientImpl<A> {
    base_url: String,
    client: reqwest::Client,
    authorizer: A,
}

impl<A> QBotApiClientImpl<A> {
    pub fn new(base_url: String, app_id: &str, authorizer: A) -> Self {
        use reqwest::header;
        let mut headers = header::HeaderMap::new();
        headers.append(
            "X-Union-Appid",
            header::HeaderValue::from_str(app_id).unwrap(),
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .default_headers(headers)
            .build()
            .unwrap();
        Self {
            base_url,
            client,
            authorizer,
        }
    }
}

trait WithAccessToken {
    async fn with_access_token(self, authorizer: impl QBotAuthorizer) -> Self;
}

impl WithAccessToken for reqwest::RequestBuilder {
    async fn with_access_token(self, authorizer: impl QBotAuthorizer) -> Self {
        let access_token = authorizer.get_access_token().await.unwrap();
        self.header("Authorization", format!("QQBot {access_token}"))
    }
}

impl<A: QBotAuthorizer + Sync> QBotApiClient for QBotApiClientImpl<A> {
    async fn reply_text_to_channel_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> QBotApiResult<()> {
        #[derive(Serialize)]
        struct ReplyTextRequest<'a> {
            msg_id: &'a str,
            content: &'a str,
        }
        #[derive(Deserialize)]
        struct ReplyTextResponse {}

        let _res: ReplyTextResponse = self
            .client
            .post(&format!("{}/channels/{channel_id}/messages", self.base_url))
            .with_access_token(&self.authorizer)
            .await
            .json(&ReplyTextRequest {
                msg_id: message_id,
                content,
            })
            .send()
            .await?
            .to_qbot_result()
            .await?;
        Ok(())
    }
}

impl<A: QBotApiClient + Sync> QBotApiClient for &A {
    async fn reply_text_to_channel_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> QBotApiResult<()> {
        (*self)
            .reply_text_to_channel_message(message_id, channel_id, content)
            .await
    }
}
impl<A: QBotApiClient + Send + Sync> QBotApiClient for std::sync::Arc<A> {
    async fn reply_text_to_channel_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> QBotApiResult<()> {
        (**self)
            .reply_text_to_channel_message(message_id, channel_id, content)
            .await
    }
}

impl<A: QBotAuthorizer + Sync> QBotApiClientImpl<A> {
    pub async fn get_ws_gateway(&self) -> QBotApiResult<String> {
        #[derive(Deserialize)]
        struct GetGatewayResponse {
            url: String,
        }
        let res: GetGatewayResponse = self
            .client
            .get(&format!("{}/gateway", self.base_url))
            .with_access_token(&self.authorizer)
            .await
            .send()
            .await?
            .to_qbot_result()
            .await?;
        Ok(res.url)
    }
}

#[cfg(test)]
mod tests {
    use mockito::Server;
    use serde_json::json;

    use crate::qbot::authorizer::MockAuthorizer;

    use super::*;

    #[tokio::test]
    async fn test_get_ws_gateway() {
        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("GET", "/gateway")
            .match_header("X-Union-Appid", "appId")
            .match_header("Authorization", "QQBot accessToken")
            .with_header("content-type", "application/json")
            .with_body(json!({ "url": "wss://example.com/ws", }).to_string())
            .create_async()
            .await;
        let client = QBotApiClientImpl::new(
            mock_server.url(),
            "appId",
            MockAuthorizer("accessToken".into()),
        );
        let res = client.get_ws_gateway().await.unwrap();
        assert_eq!(res, "wss://example.com/ws");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_reply_text_to_channel_message() {
        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("POST", "/channels/channelId/messages")
            .match_header("X-Union-Appid", "appId")
            .match_header("Authorization", "QQBot accessToken")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::Json(json!({
                "msg_id": "messageId",
                "content": "content",
            })))
            .with_header("content-type", "application/json")
            .with_body(json!({}).to_string())
            .create_async()
            .await;
        let client = QBotApiClientImpl::new(
            mock_server.url(),
            "appId",
            MockAuthorizer("accessToken".into()),
        );
        client
            .reply_text_to_channel_message("messageId", "channelId", "content")
            .await
            .unwrap();
        mock.assert_async().await;
    }
}
