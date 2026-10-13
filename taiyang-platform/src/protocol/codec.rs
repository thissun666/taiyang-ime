//! 线上帧格式: 4 字节小端长度前缀 + JSON 消息体 (抄青简 codec.rs)。这里只切帧, 传输在 app 侧。

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// 单帧上限, 挡住坏长度前缀导致的巨量分配。
const MAX_FRAME: u32 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("io: {0}")]
    Io(#[from] io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("frame too large: {0} bytes")]
    TooLarge(usize),
}

/// 写一帧: 长度前缀 + JSON, 然后 flush。
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), CodecError> {
    let body = serde_json::to_vec(value)?;
    let len = u32::try_from(body.len()).map_err(|_| CodecError::TooLarge(body.len()))?;
    if len > MAX_FRAME {
        return Err(CodecError::TooLarge(body.len()));
    }
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

/// 读一帧。对端在帧边界干净关闭返回 Ok(None); 读到一半断开算错误。
pub fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, CodecError> {
    let mut len_buf = [0u8; 4];
    if !read_fully_or_eof(reader, &mut len_buf)? {
        return Ok(None);
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME {
        return Err(CodecError::TooLarge(len as usize));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// 填满 buf。开头就 EOF 返回 Ok(false) (干净关闭); 读到一半 EOF 返回 UnexpectedEof。
fn read_fully_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed mid-frame",
                ));
            }
            read => filled += read,
        }
    }
    Ok(true)
}


impl From<CodecError> for io::Error {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::Io(io) => io,
            CodecError::Json(json) => io::Error::new(io::ErrorKind::InvalidData, json),
            CodecError::TooLarge(len) => {
                io::Error::new(io::ErrorKind::InvalidData, format!("frame too large: {len} bytes"))
            }
        }
    }
}
