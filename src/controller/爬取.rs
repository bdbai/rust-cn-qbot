use std::future::Future;

use super::{sanitizer::sanitize_message, ControllerImpl};
use crate::crawler::Crawler;

impl<A: Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) fn 爬取<'a>(&'a self, url: &'a str) -> impl Future<Output = String> + Send + 'a {
        async move {
            let Some(href) = url.strip_prefix("https://rustcc.cn") else {
                return "请输入合法的链接".into();
            };
            let post = match self.crawler.fetch_post(href).await {
                Ok(post) => post,
                Err(e) => {
                    return format!("爬取失败: {}", e);
                }
            };

            let mut gc_done_text = "";
            {
                let mut posts = self.posts.lock().unwrap();
                if posts.len() > 20 {
                    posts.clear();
                    gc_done_text = "清理完成，";
                }
            }

            let old_post = {
                let post = post.clone();
                self.posts.lock().unwrap().insert(post.date, post)
            };
            if old_post.is_some() {
                format!(
                    "{gc_done_text}重新爬取成功: {} - {}",
                    post.date,
                    sanitize_message(post.title)
                )
            } else {
                format!(
                    "{gc_done_text}爬取成功: {} - {}",
                    post.date,
                    sanitize_message(post.title)
                )
            }
        }
    }
}
