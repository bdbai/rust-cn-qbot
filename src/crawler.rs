use std::future::Future;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

use scraper::Selector;
use thiserror::Error;
use tracing::error;

use crate::post::{DailyPost, DailyPostCategory, DailyPostDate, DailyPostTitle};

#[derive(Debug, Error)]
pub enum CrawlerError {
    #[error("error sending HTTP request: {0}")]
    ConnectionError(#[from] reqwest::Error),
    #[error("unsuccessful HTTP status code: {0}")]
    HttpStatus(u16),
    #[error("error parsing HTML: {0}")]
    HtmlParseError(String),
}

pub type CrawlerResult<T> = std::result::Result<T, CrawlerError>;

pub trait Crawler {
    fn fetch_news_category(&self) -> impl Future<Output = CrawlerResult<DailyPostCategory>> + Send;
    fn fetch_post(&self, href: &str) -> impl Future<Output = CrawlerResult<DailyPost>> + Send;
}

pub struct CrawlerImpl {
    base_url: String,
    client: reqwest::Client,
}

impl CrawlerImpl {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        Self { base_url, client }
    }
}

fn parse_raw_title(title: &str) -> Option<(DailyPostDate, &str)> {
    let (_prefix, mut remaining) = title.split_once('】')?;
    remaining = remaining.trim_start();
    let date = DailyPostDate::from_str(remaining.get(..10)?).ok()?;
    let title = remaining[10..].trim();
    Some((date, title))
}

impl Crawler for CrawlerImpl {
    async fn fetch_news_category(&self) -> CrawlerResult<DailyPostCategory> {
        static ARTICLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
        static TITLE_SELECTOR: OnceLock<Selector> = OnceLock::new();

        let res = self
            .client
            .get(&format!(
                "{}/section?id=f4703117-7e6b-4caf-aa22-a3ad3db6898f",
                self.base_url
            ))
            .send()
            .await?;
        let status = res.status();
        let res_text = res.text().await?;
        if status.is_client_error() || status.is_server_error() {
            let res_text = res_text.chars().take(1024).collect::<String>();
            error!(
                "unsuccessful response code {}, response: {}",
                status.as_u16(),
                res_text
            );
            return Err(CrawlerError::HttpStatus(status.as_u16()));
        }

        let html = scraper::Html::parse_document(&res_text);
        let posts = html
            .select(ARTICLE_SELECTOR.get_or_init(|| Selector::parse(".article-list li").unwrap()))
            .filter_map(|list_node| {
                let a_node = list_node
                    .select(TITLE_SELECTOR.get_or_init(|| Selector::parse("a").unwrap()))
                    .next()?;
                let title = a_node.text().collect::<String>();
                let (date, title) = parse_raw_title(&title)?;
                let href = a_node.value().attr("href")?;
                Some(DailyPostTitle {
                    title: title.into(),
                    date,
                    href: href.into(),
                })
            })
            .collect::<Vec<_>>();
        if posts.is_empty() && !html.errors.is_empty() {
            let error = html.errors.join("");
            error!("error parsing category HTML: {:?}", error);
            return Err(CrawlerError::HtmlParseError(error));
        }
        Ok(DailyPostCategory { posts })
    }

    async fn fetch_post(&self, href: &str) -> CrawlerResult<DailyPost> {
        static CONTENT_SELECTOR: OnceLock<Selector> = OnceLock::new();
        static TITLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
        static AUTHOR_SELECTOR: OnceLock<Selector> = OnceLock::new();
        static PUBLISH_TIME_SELECTOR: OnceLock<Selector> = OnceLock::new();

        let res = self
            .client
            .get(&format!("{}{href}", self.base_url))
            .send()
            .await?;
        let status = res.status();
        let res_text = res.text().await?;
        if status.is_client_error() || status.is_server_error() {
            let res_text = res_text.chars().take(1024).collect::<String>();
            error!(
                "unsuccessful response code {}, response: {}",
                status.as_u16(),
                res_text
            );
            return Err(CrawlerError::HttpStatus(status.as_u16()));
        }

        let html = scraper::Html::parse_document(&res_text);
        let content_html = html
            .select(CONTENT_SELECTOR.get_or_init(|| Selector::parse(".detail-body > *").unwrap()))
            .map(|node| node.html())
            .collect::<Vec<_>>()
            .join("");
        if content_html.is_empty() && !html.errors.is_empty() {
            let error = html.errors.join("");
            error!("error parsing post HTML (href={}): {:?}", href, error);
            return Err(CrawlerError::HtmlParseError(error));
        }

        let title = html
            .select(
                TITLE_SELECTOR.get_or_init(|| Selector::parse(".body-content .title a").unwrap()),
            )
            .next()
            .map(|node| node.text().collect::<String>())
            .unwrap_or_default();
        let (date, title) = parse_raw_title(&title).ok_or_else(|| {
            error!("error parsing post title (href={}): {:?}", href, title);
            CrawlerError::HtmlParseError("error parsing post title".to_string())
        })?;
        let author = html
            .select(AUTHOR_SELECTOR.get_or_init(|| Selector::parse(".vice-title a").unwrap()))
            .next()
            .map(|node| node.text().collect::<String>())
            .unwrap_or_default();
        let publish_time = html
            .select(
                PUBLISH_TIME_SELECTOR
                    .get_or_init(|| Selector::parse(".vice-title .article_created_time").unwrap()),
            )
            .next()
            .map(|node| node.text().collect::<String>())
            .unwrap_or_default();

        Ok(DailyPost {
            href: href.into(),
            content_html,
            title: title.into(),
            author,
            publish_time,
            date,
        })
    }
}

impl<C: Crawler + Send + Sync> Crawler for std::sync::Arc<C> {
    async fn fetch_news_category(&self) -> CrawlerResult<DailyPostCategory> {
        (**self).fetch_news_category().await
    }
    async fn fetch_post(&self, href: &str) -> CrawlerResult<DailyPost> {
        (**self).fetch_post(href).await
    }
}

#[cfg(test)]
mod tests {
    use mockito::Server;

    use crate::post::DailyPostTitle;

    use super::*;

    #[tokio::test]
    async fn test_fetch_category() {
        let mut mock_server = Server::new_async().await;
        mock_server
            .mock("GET", "/section?id=f4703117-7e6b-4caf-aa22-a3ad3db6898f")
            .with_body(include_str!("../tests/fixtures/rustcc_category.html"))
            .create_async()
            .await;
        let crawler = CrawlerImpl::new(mock_server.url());
        let category = crawler.fetch_news_category().await.unwrap();
        assert!(category.posts.len() > 10);
        assert_eq!(
            category.posts[0],
            DailyPostTitle {
                title: "TinyUFO - 无锁高性能缓存".to_string(),
                date: "2024-04-11".parse().unwrap(),
                href: "/article?id=325542e0-9d74-47a5-ba3d-a5cb485b1b99".into(),
            }
        );
        assert_eq!(
            category.posts[1],
            DailyPostTitle {
                title: "C2PA使用Rust来实现其目标".to_string(),
                date: "2024-04-12".parse().unwrap(),
                href: "/article?id=8f907ec5-f15c-4651-9e75-58add3aaceb2".into(),
            }
        );
    }

    #[tokio::test]
    async fn test_fetch_post() {
        let mut mock_server = Server::new_async().await;
        mock_server
            .mock("GET", "/article?id=325542e0-9d74-47a5-ba3d-a5cb485b1b99")
            .with_body(include_str!(
                "../tests/fixtures/rustcc_daily_post_article.html"
            ))
            .create_async()
            .await;
        let crawler = CrawlerImpl::new(mock_server.url());
        let post = crawler
            .fetch_post("/article?id=325542e0-9d74-47a5-ba3d-a5cb485b1b99")
            .await
            .unwrap();
        assert_eq!(
            post.href,
            "/article?id=325542e0-9d74-47a5-ba3d-a5cb485b1b99"
        );
        assert_eq!(post.title, "TinyUFO - 无锁高性能缓存");
        assert_eq!(post.date, "2024-04-11".parse().unwrap());
        assert_eq!(post.author, "PsiACE");
        assert_eq!(post.publish_time, "2024-04-13 16:16");
        assert!(post.content_html.contains("TinyUFO"));
        assert!(post.content_html.contains("命中率"));
        assert!(post.content_html.contains("Hugging Face"));
        assert!(post
            .content_html
            .contains(r#"<a href="https://github.com/cloudflare/pingora/tree/main/tinyufo""#));
    }
}
