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
