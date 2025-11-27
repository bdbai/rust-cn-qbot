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

#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use crate::crawler::{CrawlerError, MockCrawler};
    use crate::post::{DailyPost, DailyPostDate};
    use crate::qbot::MockQBotApiClient;

    use super::*;

    fn make_test_post(date: &str, title: &str) -> DailyPost {
        DailyPost {
            href: "/test".into(),
            content_html: "<p>test</p>".into(),
            title: title.into(),
            author: "author".into(),
            publish_time: "2024-04-11 12:00".into(),
            date: date.parse().unwrap(),
        }
    }

    #[tokio::test]
    async fn test_爬取_invalid_url() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        crawler_mock.expect_fetch_post().never();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.爬取("http://example.com/test").await;
        assert_eq!(result, "请输入合法的链接");
    }

    #[tokio::test]
    async fn test_爬取_crawler_error() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        crawler_mock
            .expect_fetch_post()
            .with(eq("/test"))
            .times(1)
            .return_once(|_| Box::pin(async { Err(CrawlerError::HttpStatus(500)) }));

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.爬取("https://rustcc.cn/test").await;
        assert!(result.contains("爬取失败"));
        assert!(result.contains("500"));
    }

    #[tokio::test]
    async fn test_爬取_success_new_post() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        let post = make_test_post("2024-04-11", "Test Title");
        crawler_mock
            .expect_fetch_post()
            .with(eq("/test"))
            .times(1)
            .return_once(move |_| Box::pin(async move { Ok(post) }));

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.爬取("https://rustcc.cn/test").await;
        assert!(result.contains("爬取成功"));
        assert!(result.contains("2024-04-11"));
        assert!(result.contains("Test Title"));
        assert!(!result.contains("重新"));
        assert!(!result.contains("清理完成"));

        // Verify post is stored
        assert!(controller
            .posts
            .lock()
            .unwrap()
            .contains_key(&"2024-04-11".parse().unwrap()));
    }

    #[tokio::test]
    async fn test_爬取_success_update_existing_post() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        let post = make_test_post("2024-04-11", "Updated Title");
        crawler_mock
            .expect_fetch_post()
            .with(eq("/test"))
            .times(1)
            .return_once(move |_| Box::pin(async move { Ok(post) }));

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        // Pre-insert an old post
        let old_post = make_test_post("2024-04-11", "Old Title");
        controller
            .posts
            .lock()
            .unwrap()
            .insert(old_post.date, old_post);

        let result = controller.爬取("https://rustcc.cn/test").await;
        assert!(result.contains("重新爬取成功"));
        assert!(result.contains("2024-04-11"));
        assert!(result.contains("Updated Title"));
        assert!(!result.contains("清理完成"));

        // Verify post is updated
        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        let stored = controller.posts.lock().unwrap().get(&date).cloned();
        assert_eq!(stored.unwrap().title, "Updated Title");
    }

    #[tokio::test]
    async fn test_爬取_gc_triggered() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        let post = make_test_post("2024-04-11", "New Post");
        crawler_mock
            .expect_fetch_post()
            .with(eq("/test"))
            .times(1)
            .return_once(move |_| Box::pin(async move { Ok(post) }));

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        // Pre-fill with more than 20 posts to trigger GC
        {
            let mut posts = controller.posts.lock().unwrap();
            for i in 1..=25 {
                let post = make_test_post(&format!("2024-01-{:02}", i), &format!("Post {}", i));
                posts.insert(post.date, post);
            }
        }
        assert_eq!(controller.posts.lock().unwrap().len(), 25);

        let result = controller.爬取("https://rustcc.cn/test").await;
        assert!(result.contains("清理完成"));
        assert!(result.contains("爬取成功"));

        // After GC and new insert, only 1 post should remain
        assert_eq!(controller.posts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_爬取_sanitize_title() {
        let api_client_mock = MockQBotApiClient::new();
        let mut crawler_mock = MockCrawler::new();
        let post = make_test_post("2024-04-11", "Title.With.Dots");
        crawler_mock
            .expect_fetch_post()
            .return_once(move |_| Box::pin(async move { Ok(post) }));

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.爬取("https://rustcc.cn/test").await;
        assert!(result.contains("Title-With-Dots"));
        assert!(!result.contains("Title.With.Dots"));
    }
}
