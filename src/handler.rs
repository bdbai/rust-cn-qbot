use std::sync::Arc;

use regex::Regex;
use tracing::{debug, error};

use crate::qbot::ws::{payload::AtMessageCreatePayload, QBotWsMessageHandler};
use crate::qbot::QBotApiClient;

struct EventHandlerInner<A> {
    api_client: A,
}

#[derive(Clone)]
pub struct EventHandler<A> {
    inner: Arc<EventHandlerInner<A>>,
}

impl<A> EventHandler<A> {
    pub fn new(api_client: A) -> Self {
        Self {
            inner: Arc::new(EventHandlerInner { api_client }),
        }
    }
}

impl<A: QBotApiClient> EventHandlerInner<A> {
    async fn handle_at_message(&self, message: AtMessageCreatePayload) {
        const ID_WHITELIST: [&str; 1] = ["1453422017104534300"];
        if !ID_WHITELIST.contains(&message.author.id.as_str()) {
            return;
        }
        let filtered = Regex::new(r"\<\@!\d+\>")
            .unwrap()
            .replace_all(&message.content, "")
            .to_string();
        let send_res = self
            .api_client
            .reply_text_to_channel_message(&message.id, &message.channel_id, &filtered)
            .await;
        if let Err(e) = send_res {
            error!(error = %e, "failed to send message");
        }
    }
}

impl<A: QBotApiClient + Send + Sync + 'static> QBotWsMessageHandler for EventHandler<A> {
    fn handle_at_message(&mut self, message: AtMessageCreatePayload) {
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
