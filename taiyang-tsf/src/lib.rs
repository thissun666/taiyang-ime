//! 太阳输入法的 TSF 文本服务 DLL。
//! TSF 会把这个 DLL 加载进每一个应用进程, 所以 Engine 不在这里 (独立 Server 进程)。
//! DLL 只做: COM 外壳 + 按键转协议问 Server。

pub mod client;
pub mod error;

#[cfg(windows)]
mod com;

pub use error::ClientError;