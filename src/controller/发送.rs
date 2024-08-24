use std::future::Future;

use super::ControllerImpl;
use crate::crawler::Crawler;
use crate::post::DailyPostDate;
use crate::qbot::QBotApiClient;

impl<A: QBotApiClient + Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) fn 发送<'a>(
        &'a self,
        _channel_id: &'a str,
        date: DailyPostDate,
    ) -> impl Future<Output = String> + Send + 'a {
        async move {
            let post_channel_id = &*self.news_channel_id;
            let Some(post) = self.posts.lock().unwrap().get(&date).cloned() else {
                return format!("没有找到 {} 的日报", date);
            };

            let title = format!("[{}] {}", post.date, post.title.replace(".", "-"));
            let html = format!(
                r#"<p>{} 发表于 {}</p><p><a href="https://rustcc.cn{}">原文链接</a></p>{}"#,
                post.author,
                post.publish_time,
                post.href,
                post.content_html
                    .replace(r#" rel="noopener noreferrer""#, "")
            );
            let res = self
                .api_client
                .send_channel_thread_html(&post_channel_id, &title, &html)
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
