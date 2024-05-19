use std::future::Future;
use std::str::FromStr;

use super::ControllerImpl;
use crate::qbot::QBotApiClient;

impl<A: QBotApiClient + Sync, C: Sync> ControllerImpl<A, C> {
    pub(super) fn 设置频道<'a>(
        &'a self,
        channel_id: &'a str,
    ) -> impl Future<Output = String> + Send + 'a {
        async move {
            if u64::from_str(channel_id).is_err() {
                return "频道 ID 必须是数字".to_string();
            }
            {
                let mut guard = self.channel_id.lock().unwrap();
                *guard = Some(channel_id.into());
            }
            "设置成功".to_string()
        }
    }
}
