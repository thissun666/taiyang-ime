//! TSF 侧日志: %LOCALAPPDATA%\Taiyang\logs\tsf.log (追加)。
//! 注: 青简 log.rs 未在 dump 里, 这是自写最小实现 (多进程并发小段追加, 调试用足够)。

use std::io::Write;
use std::sync::{Mutex, OnceLock};

static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

pub(crate) fn log(msg: &str) {
    let sink = SINK.get_or_init(|| {
        let dir = taiyang_platform::dirs::log_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("tsf.log"))
            .ok()?;
        Some(Mutex::new(file))
    });
    if let Some(mutex) = sink.as_ref() {
        if let Ok(mut file) = mutex.lock() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[{now}] {msg}");
        }
    }
}