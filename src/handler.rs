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
