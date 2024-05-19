use std::future::Future;

use super::ControllerImpl;
use crate::crawler::Crawler;
use crate::post::{DailyPost, DailyPostDate};
use crate::qbot::QBotApiClient;

impl<A: QBotApiClient + Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) fn 发送<'a>(
        &'a self,
        _channel_id: &'a str,
        date: DailyPostDate,
    ) -> impl Future<Output = String> + Send + 'a {
        async move {
            let post_channel_id = match {
                let channel_id = self.channel_id.lock().unwrap();
                channel_id.clone()
            } {
                Some(id) => id,
                None => {
                    "651407771".into()
                    // return "请先设置频道".to_string();
                }
            };
            let post = match self.posts.lock().unwrap().get(&date).cloned() {
                Some(post) => post,
                None => {
                    // return format!("没有找到 {} 的日报", date);
                    DailyPost {
                        href:"/".into(),
                        date: "2021-01-01".parse().unwrap(),
                        title: "测试".into(),
                        author:"".into(),
                        publish_time: "".into(),
                        content_html: r#"<html lang="en-US"><body><a href="https://bot.q.qq.com/wiki" title="QQ机器人文档Title">QQ机器人文档</a>
<ul><li>主动消息：发送消息时，未填msg_id字段的消息。</li><li>被动消息：发送消息时，填充了msg_id字段的消息。</li></ul></body></html>"#.into(),
                    }
                }
            };

            let title = format!("[{}] {}", post.date, post.title);
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
