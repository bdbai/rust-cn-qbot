use std::fmt::{Debug, Display};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct OpCode(pub u8);

impl OpCode {
    pub(super) const OP_DISPATCH: OpCode = OpCode(0);
    pub(super) const OP_HEARTBEAT: OpCode = OpCode(1);
    pub(super) const OP_IDENTIFY: OpCode = OpCode(2);
    pub(super) const OP_RESUME: OpCode = OpCode(6);
    pub(super) const OP_RECONNECT: OpCode = OpCode(7);
    pub(super) const OP_INVALID_SESSION: OpCode = OpCode(9);
    pub(super) const OP_HELLO: OpCode = OpCode(10);
    pub(super) const OP_HEARTBEAT_ACK: OpCode = OpCode(11);
    pub(super) const OP_HTTP_CALLBACK_ACK: OpCode = OpCode(12);
    pub(super) const OP_HTTP_CALLBACK_CHALLENGE: OpCode = OpCode(13);

    fn try_get_name(&self) -> Option<&'static str> {
        Some(match *self {
            Self::OP_DISPATCH => "Dispatch",
            Self::OP_HEARTBEAT => "Heartbeat",
            Self::OP_IDENTIFY => "Identify",
            Self::OP_RESUME => "Resume",
            Self::OP_HELLO => "Hello",
            Self::OP_HEARTBEAT_ACK => "HeartbeatAck",
            _ => return None,
        })
    }
}

pub(super) trait OpCodePayload {
    const OPCODE: OpCode;
}

impl Debug for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.try_get_name() {
            Some(name) => write!(f, "{}", name),
            None => write!(f, "Op({})", self.0),
        }
    }
}

impl Display for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as Debug>::fmt(self, f)
    }
}
