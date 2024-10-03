use std::cell::RefCell;
use std::future::Future;
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
                process_elements(&mut *child.children.borrow_mut());
            }
        }
        process_elements(&mut *children);
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
                .send_channel_thread_html(&post_channel_id, &title, &html)
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
