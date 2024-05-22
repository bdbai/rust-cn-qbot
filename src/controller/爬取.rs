use std::future::Future;

use super::ControllerImpl;
use crate::crawler::Crawler;

impl<A: Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) fn 爬取<'a>(&'a self, href: &'a str) -> impl Future<Output = String> + Send + 'a {
        async move {
            if !href.starts_with('/') {
                return "请输入合法的相对链接，以/开头，不包含域名".into();
            }
            let post = match self.crawler.fetch_post(href).await {
                Ok(post) => post,
                Err(e) => {
                    return format!("爬取失败: {}", e);
                }
            };

            let old_post = {
                let post = post.clone();
                self.posts.lock().unwrap().insert(post.date, post)
            };
            if old_post.is_some() {
                format!("重新爬取成功: {} - {}", post.date, post.title)
            } else {
                format!("爬取成功: {} - {}", post.date, post.title)
            }
        }
    }
}
