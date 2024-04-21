use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::opcode::{OpCode, OpCodePayload};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct QBotWebSocketAnyPayload {
    #[serde(rename = "op")]
    pub opcode: OpCode,
    #[serde(rename = "s")]
    pub seq: Option<i32>,
    #[serde(rename = "t")]
    pub event_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct QBotWebSocketPayload<D> {
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
    #[serde(rename = "bot")]
    pub is_bot: bool,
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtMessageCreateMember {
    pub joined_at: String,
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
