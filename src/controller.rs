use std::sync::Mutex;
use std::{collections::BTreeMap, future::Future};

#[path = "controller/发送.rs"]
mod 发送;
#[path = "controller/所有频道.rs"]
mod 所有频道;
#[path = "controller/爬取.rs"]
mod 爬取;

use crate::crawler::Crawler;
use crate::post::{DailyPost, DailyPostDate};
use crate::qbot::QBotApiClient;

pub trait Controller {
    fn 所有频道(&self, guild_id: &str) -> impl Future<Output = String> + Send;
    fn 爬取(&self, href: &str) -> impl Future<Output = String> + Send;
    fn 发送(&self, channel_id: &str, date: DailyPostDate) -> impl Future<Output = String> + Send;
}

pub struct ControllerImpl<A, C> {
    crawler: C,
    posts: Mutex<BTreeMap<DailyPostDate, DailyPost>>,
    news_channel_id: String,
    api_client: A,
}

impl<A, C> ControllerImpl<A, C> {
    pub fn new(api_client: A, crawler: C, news_channel_id: String) -> Self {
        Self {
            crawler,
            posts: Default::default(),
            news_channel_id,
            api_client,
        }
    }
}

impl<A: QBotApiClient + Sync, C: Crawler + Sync> Controller for ControllerImpl<A, C> {
    fn 所有频道(&self, guild_id: &str) -> impl Future<Output = String> + Send {
        async { self.所有频道(guild_id).await }
    }

    fn 爬取(&self, href: &str) -> impl Future<Output = String> + Send {
        async { self.爬取(href).await }
    }

    fn 发送(&self, channel_id: &str, date: DailyPostDate) -> impl Future<Output = String> + Send {
        async move { self.发送(channel_id, date).await }
    }
}
