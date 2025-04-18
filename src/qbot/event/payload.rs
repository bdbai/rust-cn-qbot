use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::opcode::{OpCode, OpCodePayload};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct QBotEventAnyPayload {
    #[serde(rename = "op")]
    pub opcode: OpCode,
    #[serde(rename = "s")]
    pub seq: Option<i32>,
    #[serde(rename = "t")]
    pub event_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct QBotEventPayload<D> {
    #[serde(rename = "op")]
    pub opcode: OpCode,
    #[serde(rename = "d")]
    pub data: D,
    #[serde(rename = "s")]
    pub seq: Option<i32>,
    #[serde(rename = "t")]
    pub event_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HelloPayload {
    pub heartbeat_interval: u64,
}
impl OpCodePayload for HelloPayload {
    const OPCODE: OpCode = OpCode::OP_HELLO;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentifyPayload<'a> {
    pub token: &'a str,
    pub intents: u64,
    pub shard: (u32, u32),
    pub properties: BTreeMap<String, String>,
}
impl OpCodePayload for IdentifyPayload<'_> {
    const OPCODE: OpCode = OpCode::OP_IDENTIFY;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyUser {
    pub id: String,
    pub username: String,
    pub bot: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyPayload {
    pub version: u32,
    pub session_id: String,
    pub user: ReadyUser,
    pub shard: (u32, u32),
}
impl OpCodePayload for ReadyPayload {
    const OPCODE: OpCode = OpCode::OP_DISPATCH;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatPayload;
impl OpCodePayload for HeartbeatPayload {
    const OPCODE: OpCode = OpCode::OP_HEARTBEAT;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatAckPayload;
impl OpCodePayload for Option<HeartbeatAckPayload> {
    const OPCODE: OpCode = OpCode::OP_HEARTBEAT_ACK;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePayload<'a> {
    pub token: &'a str,
    pub session_id: &'a str,
    pub seq: i32,
}
impl OpCodePayload for ResumePayload<'_> {
    const OPCODE: OpCode = OpCode::OP_RESUME;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtMessageCreateAuthor {
    #[serde(rename = "avatar")]
    pub avatar_url: String,
    #[serde(default)]
    pub is_bot: Option<bool>,
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtMessageCreateMember {
    pub joined_at: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtMessageCreatePayload {
    pub author: AtMessageCreateAuthor,
    pub channel_id: String,
    pub content: String,
    pub guild_id: String,
    pub id: String,
    pub member: AtMessageCreateMember,
    pub timestamp: String,
    pub seq: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessageCreatePayload {
    pub author: AtMessageCreateAuthor,
    pub channel_id: String,
    pub content: String,
    pub guild_id: String,
    pub id: String,
    pub member: AtMessageCreateMember,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookChallengePayload {
    pub plain_token: String,
    pub event_ts: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookChallengeResponsePayload<'a> {
    pub plain_token: &'a str,
    pub signature: &'a str,
}
