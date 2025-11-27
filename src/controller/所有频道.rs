use std::future::Future;

use super::ControllerImpl;
use crate::qbot::QBotApiClient;

impl<A: QBotApiClient + Sync, C: Sync> ControllerImpl<A, C> {
    pub(super) fn 所有频道<'a>(
        &'a self,
        guild_id: &'a str,
    ) -> impl Future<Output = String> + Send + 'a {
        async move {
            let channels = match self.api_client.list_channels(guild_id).await {
                Ok(channels) => channels,
                Err(e) => {
                    return format!("获取频道列表失败: {e}");
                }
            };
            let channel_desc = channels
                .into_iter()
                .map(|c| format!("{} {}", c.id, c.name))
                .collect::<Vec<_>>()
                .join("; ");
            channel_desc
        }
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use crate::crawler::MockCrawler;
    use crate::qbot::{Channel, MockQBotApiClient, QBotApiError};

    use super::*;

    #[tokio::test]
    async fn test_所有频道_api_error() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_list_channels()
            .with(eq("guild123"))
            .times(1)
            .return_once(|_| {
                Box::pin(async {
                    Err(QBotApiError::ApiError {
                        status_code: 403,
                        code: 10001,
                        message: "no permission".into(),
                        trace_id: "trace".into(),
                    })
                })
            });
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.所有频道("guild123").await;
        assert!(result.contains("获取频道列表失败"));
    }

    #[tokio::test]
    async fn test_所有频道_empty_list() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_list_channels()
            .with(eq("guild123"))
            .times(1)
            .return_once(|_| Box::pin(async { Ok(vec![]) }));
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.所有频道("guild123").await;
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_所有频道_success() {
        let mut api_client_mock = MockQBotApiClient::new();
        api_client_mock
            .expect_list_channels()
            .with(eq("guild123"))
            .times(1)
            .return_once(|_| {
                Box::pin(async {
                    Ok(vec![
                        Channel {
                            id: "ch1".into(),
                            guild_id: "guild123".into(),
                            name: "频道一".into(),
                        },
                        Channel {
                            id: "ch2".into(),
                            guild_id: "guild123".into(),
                            name: "频道二".into(),
                        },
                    ])
                })
            });
        let crawler_mock = MockCrawler::new();

        let controller = ControllerImpl::new(api_client_mock, crawler_mock, "news".into());

        let result = controller.所有频道("guild123").await;
        assert_eq!(result, "ch1 频道一; ch2 频道二");
    }
}
