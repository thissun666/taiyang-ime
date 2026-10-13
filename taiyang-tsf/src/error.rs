/// 与 Server 通信时的错误 (M2 起扩充)。
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// 等应答时对端在帧边界关闭了连接。
    #[error("server closed the connection")]
    Closed,
}