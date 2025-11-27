use std::cell::RefCell;
use std::rc::Rc;

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, namespace_url, ns, parse_fragment, serialize, QualName};
use markup5ever_rcdom::{Node, NodeData, RcDom, SerializableHandle};
use tracing::warn;

use super::ControllerImpl;
use crate::controller::sanitizer::sanitize_message;
use crate::crawler::Crawler;
use crate::post::DailyPostDate;
use crate::qbot::QBotApiClient;

fn process_html(html: &str) -> Result<String, &'static str> {
    let dom = parse_fragment(
        RcDom::default(),
        Default::default(),
        QualName::new(None, ns!(), local_name!("body")),
        vec![],
    )
    .from_utf8()
    .read_from(&mut html.as_bytes())
    .map_err(|_| "解析 HTML 失败")?;
    {
        let mut children = dom.document.children.borrow_mut();
        fn replace_with_div_text(node: &mut Rc<Node>, replace_text: &str) {
            let el_data = Node::new(NodeData::Element {
                name: QualName::new(None, ns!(), local_name!("div")),
                attrs: Default::default(),
                template_contents: Default::default(),
                mathml_annotation_xml_integration_point: false,
            });
            el_data
                .children
                .borrow_mut()
                .push(Node::new(NodeData::Text {
                    contents: RefCell::new(replace_text.into()),
                }));
            *node = el_data;
        }
        fn process_elements(nodes: &mut [Rc<Node>]) {
            for child in nodes {
                match &child.data {
                    NodeData::Element { name, .. }
                        if name.local.eq_str_ignore_ascii_case("img") =>
                    {
                        replace_with_div_text(child, "（此处应有图片，请前往原文链接查看）");
                    }
                    NodeData::Element { name, .. }
                        if name.local.eq_str_ignore_ascii_case("pre") =>
                    {
                        replace_with_div_text(child, "（此处应有代码块，请前往原文链接查看）");
                    }
                    NodeData::Element { name, attrs, .. }
                        if name.local.eq_str_ignore_ascii_case("a") =>
                    {
                        attrs.borrow_mut().retain(|attr| &*attr.name.local != "rel");
                    }
                    _ => {}
                }
                process_elements(&mut child.children.borrow_mut());
            }
        }
        process_elements(&mut children);
    }
    let mut output = Vec::with_capacity(html.len());
    for child in dom.document.children.borrow_mut().drain(..) {
        let handle: SerializableHandle = child.into();
        serialize(&mut output, &handle, Default::default())
            .expect("failed to serialize HTML to String");
    }
    String::from_utf8(output).or_else(|e| Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

impl<A: QBotApiClient + Sync, C: Crawler + Sync> ControllerImpl<A, C> {
    pub(super) async fn 发送<'a>(&'a self, _channel_id: &'a str, date: DailyPostDate) -> String {
        let post_channel_id = &*self.news_channel_id;
        let Some(post) = self.posts.lock().unwrap().get(&date).cloned() else {
            return format!("没有找到 {} 的日报", date);
        };

        let title = format!("[{}] {}", post.date, post.title);
        let mut content_html = &post.content_html;
        let processed_html = process_html(content_html);
        let mut process_error = String::new();
        content_html = match &processed_html {
            Ok(html) => html,
            Err(e) => {
                warn!("Failed to process HTML: {}", e);
                process_error = format!(" （HTML 处理失败:{e}）");
                content_html
            }
        };
        let html = format!(
            r#"<p>{} 发表于 {}</p><p><a href="https://rustcc.cn{}">原文链接</a></p>{}"#,
            post.author, post.publish_time, post.href, content_html
        );
        let res = self
            .api_client
            .send_channel_thread_html(post_channel_id, &title, &html)
            .await;
        match res {
            Ok(_) => {
                self.posts.lock().unwrap().remove(&date);
                format!(
                    "发送成功: {} - {}{process_error}",
                    post.date,
                    sanitize_message(post.title)
                )
            }
            Err(e) => format!("发送失败: {}", sanitize_message(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use crate::crawler::MockCrawler;
    use crate::post::DailyPost;
    use crate::qbot::{MockQBotApiClient, QBotApiError};

    use super::*;

    fn make_test_post(date: &str, title: &str) -> DailyPost {
        DailyPost {
            href: "/test-href".into(),
            content_html: "<p>test content</p>".into(),
            title: title.into(),
            author: "TestAuthor".into(),
            publish_time: "2024-04-11 12:00".into(),
            date: date.parse().unwrap(),
        }
    }

    #[test]
    fn test_html_replacement() {
        let res = process_html(
            r#"<div>内容：<a rel="relval"><img src="aa"></a><pre></pre></div><div>div2</div>"#,
        );
        let res = res.unwrap();
        println!("{res}");
        let contains_img = res.contains("<img");
        let contains_pre = res.contains("<pre");
        let contains_relval = res.contains("relval");
        let contains_img_replacement = res.contains("（此处应有图片，请前往原文链接查看）");
        let contains_pre_replacement = res.contains("（此处应有代码块，请前往原文链接查看）");
        assert!(!res.contains("<html"));
        assert!(res.contains("内容"));
        assert!(res.contains("div2"));
        assert_eq!(
            (contains_img, contains_pre, contains_relval),
            (false, false, false)
        );
        assert_eq!(
            (contains_img_replacement, contains_pre_replacement),
            (true, true)
        );
    }

    #[tokio::test]
    async fn test_发送_post_not_found() {
        let api_client_mock = MockQBotApiClient::new();
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news_channel".into());

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        let result = controller.发送("channel", date).await;
        assert!(result.contains("没有找到"));
        assert!(result.contains("2024-04-11"));
    }

    #[tokio::test]
    async fn test_发送_api_error() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_send_channel_thread_html()
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
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news_channel".into());

        // Pre-insert a post
        let post = make_test_post("2024-04-11", "Test Title");
        controller.posts.lock().unwrap().insert(post.date, post);

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        let result = controller.发送("channel", date).await;
        assert!(result.contains("发送失败"));

        // Post should still be there
        assert!(controller.posts.lock().unwrap().contains_key(&date));
    }

    #[tokio::test]
    async fn test_发送_success() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_send_channel_thread_html()
            .times(1)
            .withf(|channel_id, title, html| {
                channel_id == "news_channel"
                    && title == "[2024-04-11] Test Title"
                    && html.contains("TestAuthor")
                    && html.contains("2024-04-11 12:00")
                    && html.contains("https://rustcc.cn/test-href")
                    && html.contains("test content")
            })
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news_channel".into());

        // Pre-insert a post
        let post = make_test_post("2024-04-11", "Test Title");
        controller.posts.lock().unwrap().insert(post.date, post);

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        let result = controller.发送("channel", date).await;
        assert!(result.contains("发送成功"));
        assert!(result.contains("2024-04-11"));
        assert!(result.contains("Test Title"));

        // Post should be removed after successful send
        assert!(!controller.posts.lock().unwrap().contains_key(&date));
    }

    #[tokio::test]
    async fn test_发送_uses_news_channel_id() {
        // Test that it uses the configured news_channel_id, not the channel_id parameter
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_send_channel_thread_html()
            .times(1)
            .with(eq("configured_news_channel"), always(), always())
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(
            api_client_mock,
            crawler_mock,
            "configured_news_channel".into(),
        );

        let post = make_test_post("2024-04-11", "Test");
        controller.posts.lock().unwrap().insert(post.date, post);

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        // Note: the channel_id parameter is ignored, news_channel_id is used instead
        controller.发送("different_channel", date).await;
    }

    #[tokio::test]
    async fn test_发送_sanitizes_title() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_send_channel_thread_html()
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news_channel".into());

        // Pre-insert a post with dots in title
        let post = make_test_post("2024-04-11", "Title.With.Dots");
        controller.posts.lock().unwrap().insert(post.date, post);

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        let result = controller.发送("channel", date).await;
        assert!(result.contains("Title-With-Dots"));
        assert!(!result.contains("Title.With.Dots"));
    }

    #[tokio::test]
    async fn test_发送_html_processing() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_send_channel_thread_html()
            .times(1)
            .withf(|_, _, html| {
                // img and pre should be replaced
                !html.contains("<img") && !html.contains("<pre")
            })
            .return_once(|_, _, _| Box::pin(async { Ok(()) }));
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news_channel".into());

        // Pre-insert a post with img and pre tags
        let mut post = make_test_post("2024-04-11", "Test");
        post.content_html = r#"<p>Text</p><img src="test.png"><pre>code</pre>"#.into();
        controller.posts.lock().unwrap().insert(post.date, post);

        let date: DailyPostDate = "2024-04-11".parse().unwrap();
        controller.发送("channel", date).await;
    }
}
