//! 用户目录约定 (抄青简 dirs.rs): 数据 %APPDATA%\Taiyang; 日志 %LOCALAPPDATA%\Taiyang\logs。
//! 只查环境变量, 不碰系统 API。

use std::path::PathBuf;

/// 用户数据目录 %APPDATA%\Taiyang。
pub fn user_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("Taiyang"))
}

/// 配置文件 %APPDATA%\Taiyang\config.toml。
pub fn config_path() -> Option<PathBuf> {
    user_dir().map(|dir| dir.join("config.toml"))
}

/// 运行日志目录 %LOCALAPPDATA%\Taiyang\logs, 不负责创建。
pub fn log_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Taiyang").join("logs"))
}