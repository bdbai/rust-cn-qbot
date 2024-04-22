use std::future::Future;

use super::ControllerImpl;
use crate::crawler::Crawler;
use crate::post::DailyPostDate;
use crate::qbot::QBotApiClient;

impl<A: QBotApiClient + Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) fn 发送<'a>(
        &'a self,
        channel_id: &'a str,
        date: DailyPostDate,
    ) -> impl Future<Output = String> + Send + 'a {
        async move {
            let post = match self.posts.lock().unwrap().get(&date).cloned() {
                Some(post) => post,
                None => {
                    return format!("没有找到 {} 的日报", date);
                }
            };

            let res = self
                .api_client
                .send_channel_thread_html(channel_id, &post.title, &post.content_html)
                .await;
            match res {
                Ok(_) => {
                    self.posts.lock().unwrap().remove(&date);
                    format!("发送成功: {} - {}", post.date, post.title)
                }
                Err(e) => format!("发送失败: {}", e),
            }
        }
    }
}
