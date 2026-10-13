use serde::{Deserialize, Serialize};

use super::key::KeyEvent;

/// DLL (客户端, 每个应用进程里一个) 发给 Server 的消息。M2 子集。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// 进入一个文档/线程, 开一个会话。
    OpenSession {
        /// 会话标识 (DLL 侧 = 线程 id)。
        session: u64,

        /// DLL 编译时的 PROTOCOL_VERSION。Server 对不上只记警告照常服务。
        #[serde(default)]
        protocol: u32,
    },

    /// 一次按键, 等 Server 回 KeyResult。
    Key {
        session: u64,
        event: KeyEvent,
    },

    /// 结束组句。M2 尚无组句缓冲, 保留形状给 M3。
    Commit {
        session: u64,
    },
}