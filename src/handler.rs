use std::sync::Arc;

use regex::Regex;
use tracing::{debug, error, info};

use crate::controller::Controller;
use crate::qbot::event::{payload::AtMessageCreatePayload, QBotEventMessageHandler};
use crate::qbot::QBotApiClient;

struct EventHandlerInner<A, C> {
    api_client: A,
    controller: C,
}

pub struct EventHandler<A, C> {
    inner: Arc<EventHandlerInner<A, C>>,
}

impl<A, C> Clone for EventHandler<A, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<A, C> EventHandler<A, C> {
    pub fn new(api_client: A, controller: C) -> Self {
        Self {
            inner: Arc::new(EventHandlerInner {
                api_client,
                controller,
            }),
        }
    }
}

impl<A: QBotApiClient, C: Controller> EventHandlerInner<A, C> {
    async fn handle_at_message(&self, message: AtMessageCreatePayload) {
        const ID_WHITELIST: [&str; 1] = ["1453422017104534300"];
        if !ID_WHITELIST.contains(&message.author.id.as_str()) {
            info!(%message.author.id, "not in whitelist, ignore");
            return;
        }
        let filtered = Regex::new(r"<@!\d+>")
            .unwrap()
            .replace_all(&message.content, "")
            .to_string();
        let mut filtered = filtered.trim();
        filtered = filtered.trim_start_matches('/').trim();
        debug!(filtered = %filtered, "got filtered message");
        let reply_msg = if let Some(href) = filtered.strip_prefix("爬取") {
            self.controller.爬取(href.trim()).await
        } else if let Some(date) = filtered.strip_prefix("发送") {
            let date = date.trim().parse();
            if let Ok(date) = date {
                self.controller.发送(&message.channel_id, date).await
            } else {
                "无效的日期格式".into()
            }
        } else if filtered == "所有频道" {
            self.controller.所有频道(&message.guild_id).await
        } else if filtered == "帮助" {
            "爬取 <链接> - 爬取指定链接的文章\n发送 <日期> - 发送指定日期的文章".into()
        } else {
            "不支持的命令".into()
        };
        let send_res = self
            .api_client
            .reply_text_to_channel_message(&message.id, &message.channel_id, &reply_msg)
            .await;
        if let Err(e) = send_res {
            error!(error = %e, "failed to send message");
        }
    }
}

impl<A: QBotApiClient + Send + Sync + 'static, C: Controller + Send + Sync + 'static>
    QBotEventMessageHandler for EventHandler<A, C>
{
    fn handle_at_message(&self, message: AtMessageCreatePayload) {
        debug!(
            name: "received at message",
            content=%message.content,
            %message.author.id,
            %message.author.username,
            %message.channel_id,
            %message.guild_id);
        let inner = self.inner.clone();
        tokio::spawn(async move {
            inner.handle_at_message(message).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use crate::controller::MockController;
    use crate::qbot::event::payload::AtMessageCreateAuthor;
    use crate::qbot::MockQBotApiClient;

    use super::*;

    const AUTHORIZED_ID: &str = "1453422017104534300";

    #[tokio::test]
    async fn test_handle_at_message_reject_unwhitelisted() {
        let mut controller_mock = MockController::new();
        controller_mock.expect_发送().never();
        controller_mock.expect_爬取().never();
        controller_mock.expect_所有频道().never();
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_reply_text_to_channel_message()
            .never();

        let handler = EventHandler::new(api_client_mock, controller_mock);
        let message = AtMessageCreatePayload {
            author: AtMessageCreateAuthor {
                id: "unauthorized_id".into(),
                ..Default::default()
            },
            ..Default::default()
        };

        handler.inner.handle_at_message(message).await;
    }

    #[tokio::test]
    async fn test_handle_at_message_爬取() {
        for cmd in &[
            "爬取 http://example.com",
            "爬取    http://example.com   ",
            "/爬取  <@!23897938>   http://example.com",
        ] {
            let mut controller_mock = MockController::new();
            controller_mock
                .expect_爬取()
                .with(eq("http://example.com"))
                .times(1)
                .return_once(|_| Box::pin(async { "爬取结果".into() }));
            controller_mock.expect_发送().never();
            controller_mock.expect_所有频道().never();
            let mut api_client_mock = MockQBotApiClient::new();
            api_client_mock
                .expect_reply_text_to_channel_message()
                .times(1)
                .with(eq("messageId"), eq("channelId"), eq("爬取结果"))
                .return_once(|_, _, _| Box::pin(async { Ok(()) }));

            let handler = EventHandler::new(api_client_mock, controller_mock);
            let message = AtMessageCreatePayload {
                content: cmd.to_string(),
                id: "messageId".into(),
                channel_id: "channelId".into(),
                author: AtMessageCreateAuthor {
                    id: AUTHORIZED_ID.into(),
                    ..Default::default()
                },
                ..Default::default()
            };
            handler.inner.handle_at_message(message).await;
        }
    }

    #[tokio::test]
    async fn test_handle_at_message_发送_valid_date() {
        for cmd in &[
            "发送 2024-04-11",
            "发送    2024-04-11   ",
            "/发送  <@!23897938>   2024-04-11",
        ] {
            let mut controller_mock = MockController::new();
            controller_mock.expect_爬取().never();
            let date: crate::post::DailyPostDate = "2024-04-11".parse().unwrap();
            controller_mock
                .expect_发送()
                .with(eq("channelId"), eq(date))
                .times(1)
                .return_once(|_, _| Box::pin(async { "发送结果".into() }));
            controller_mock.expect_所有频道().never();
            let mut api_client_mock = MockQBotApiClient::new();
            api_client_mock
                .expect_reply_text_to_channel_message()
                .times(1)
                .with(eq("messageId"), eq("channelId"), eq("发送结果"))
                .return_once(|_, _, _| Box::pin(async { Ok(()) }));

            let handler = EventHandler::new(api_client_mock, controller_mock);
            let message = AtMessageCreatePayload {
                content: cmd.to_string(),
                id: "messageId".into(),
                channel_id: "channelId".into(),
                author: AtMessageCreateAuthor {
                    id: AUTHORIZED_ID.into(),
                    ..Default::default()
                },
                ..Default::default()
            };
            handler.inner.handle_at_message(message).await;
        }
    }

    #[tokio::test]
    async fn test_handle_at_message_发送_invalid_date() {
        let mut controller_mock = MockController::new();
        controller_mock.expect_爬取().never();
        controller_mock.expect_发送().never();
        controller_mock.expect_所有频道().never();
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_reply_text_to_channel_message()
            .times(1)
            .with(eq("messageId"), eq("channelId"), eq("无效的日期格式"))
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));

        let handler = EventHandler::new(api_client_mock, controller_mock);
        let message = AtMessageCreatePayload {
            content: "发送 invalid-date".into(),
            id: "messageId".into(),
            channel_id: "channelId".into(),
            author: AtMessageCreateAuthor {
                id: AUTHORIZED_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        handler.inner.handle_at_message(message).await;
    }

    #[tokio::test]
    async fn test_handle_at_message_所有频道() {
        for cmd in &[
            "所有频道",
            "  所有频道  ",
            "/所有频道",
            "<@!23897938> 所有频道",
        ] {
            let mut controller_mock = MockController::new();
            controller_mock.expect_爬取().never();
            controller_mock.expect_发送().never();
            controller_mock
                .expect_所有频道()
                .with(eq("guildId"))
                .times(1)
                .return_once(|_| Box::pin(async { "频道列表".into() }));
            let mut api_client_mock = MockQBotApiClient::new();
            api_client_mock
                .expect_reply_text_to_channel_message()
                .times(1)
                .with(eq("messageId"), eq("channelId"), eq("频道列表"))
                .return_once(|_, _, _| Box::pin(async { Ok(()) }));

            let handler = EventHandler::new(api_client_mock, controller_mock);
            let message = AtMessageCreatePayload {
                content: cmd.to_string(),
                id: "messageId".into(),
                channel_id: "channelId".into(),
                guild_id: "guildId".into(),
                author: AtMessageCreateAuthor {
                    id: AUTHORIZED_ID.into(),
                    ..Default::default()
                },
                ..Default::default()
            };
            handler.inner.handle_at_message(message).await;
        }
    }

    #[tokio::test]
    async fn test_handle_at_message_帮助() {
        let mut controller_mock = MockController::new();
        controller_mock.expect_爬取().never();
        controller_mock.expect_发送().never();
        controller_mock.expect_所有频道().never();
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_reply_text_to_channel_message()
            .times(1)
            .with(
                eq("messageId"),
                eq("channelId"),
                eq("爬取 <链接> - 爬取指定链接的文章\n发送 <日期> - 发送指定日期的文章"),
            )
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));

        let handler = EventHandler::new(api_client_mock, controller_mock);
        let message = AtMessageCreatePayload {
            content: "帮助".into(),
            id: "messageId".into(),
            channel_id: "channelId".into(),
            author: AtMessageCreateAuthor {
                id: AUTHORIZED_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        handler.inner.handle_at_message(message).await;
    }

    #[tokio::test]
    async fn test_handle_at_message_unsupported_command() {
        let mut controller_mock = MockController::new();
        controller_mock.expect_爬取().never();
        controller_mock.expect_发送().never();
        controller_mock.expect_所有频道().never();
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_reply_text_to_channel_message()
            .times(1)
            .with(eq("messageId"), eq("channelId"), eq("不支持的命令"))
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));

        let handler = EventHandler::new(api_client_mock, controller_mock);
        let message = AtMessageCreatePayload {
            content: "未知命令".into(),
            id: "messageId".into(),
            channel_id: "channelId".into(),
            author: AtMessageCreateAuthor {
                id: AUTHORIZED_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        handler.inner.handle_at_message(message).await;
    }

    #[tokio::test]
    async fn test_handle_at_message_reply_error() {
        use crate::qbot::QBotApiError;

        let mut controller_mock = MockController::new();
        controller_mock
            .expect_爬取()
            .return_once(|_| Box::pin(async { "爬取结果".into() }));
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_reply_text_to_channel_message()
            .times(1)
            .return_once(|_, _, _| {
                Box::pin(async {
                    Err(QBotApiError::ApiError {
                        status_code: 500,
                        code: 1001,
                        message: "server error".into(),
                        trace_id: "trace".into(),
                    })
                })
            });

        let handler = EventHandler::new(api_client_mock, controller_mock);
        let message = AtMessageCreatePayload {
            content: "爬取 http://example.com".into(),
            id: "messageId".into(),
            channel_id: "channelId".into(),
            author: AtMessageCreateAuthor {
                id: AUTHORIZED_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        // Should not panic, just log the error
        handler.inner.handle_at_message(message).await;
    }
}
