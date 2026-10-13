//! Server <-> TSF DLL 的 IPC 协议 (M2 子集)。传输是 TCP 回环, 帧格式与传输无关,
//! M5 后换命名管道只动传输层。一问一答: DLL 每发一条 ClientMessage, 阻塞等一条 ServerMessage。
//!
//! 纪律 (抄青简血泪): 加枚举变体不在「仍能对话」之列——对端解不出会整帧失败、按键直接放行。
//! 加变体/改语义必须 PROTOCOL_VERSION +1。

mod client;
mod codec;
mod key;
mod server;

/// 协议版本, DLL 开会话时带上。
pub const PROTOCOL_VERSION: u32 = 4;

pub use client::ClientMessage;
pub use codec::{CodecError, read_message, write_message};
pub use key::{KeyEvent, KeyModifiers, KeyOutcome};
pub use server::{Candidate, ServerMessage};