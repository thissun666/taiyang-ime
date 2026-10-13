//! DLL 侧的 Server 客户端。Activate 时 connect + OpenSession, 按键时发 Key 等 KeyResult。
//! 铁律 (抄青简): 任何通信失败一律按键放行——宁可整个退回直通, 不能吞用户的键。

use std::io;
use std::net::TcpStream;
use std::path::PathBuf;

use taiyang_platform::protocol::{
    Candidate,
    ClientMessage, KeyModifiers, KeyEvent, KeyOutcome, ServerMessage, PROTOCOL_VERSION,
    read_message, write_message,
};

/// 一个会话的连接。broken 之后恒放行, 不重连 (重连等下次 Activate)。
pub struct TsClient {
    stream: TcpStream,
    session: u64,

    /// 通信失败过一次就短路, 后续全部直通。
    broken: bool,
}

impl TsClient {
    /// 连 Server 并开会话。Server 没跑 = 立刻 Err, 调用方记日志后保持 None。
    pub fn connect(session: u64) -> io::Result<Self> {
        let port = read_port_file()?;
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        write_message(
            &mut stream,
            &ClientMessage::OpenSession {
                session,
                protocol: PROTOCOL_VERSION,
            },
        )?;
        let reply: Option<ServerMessage> = read_message(&mut stream)?;
        match reply {
            Some(ServerMessage::SessionOpened { .. }) => Ok(Self {
                stream,
                session,
                broken: false,
            }),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "会话应答异常")),
        }
    }

    /// 问 Server 一次按键。任何失败 -> Passthrough 并标记 broken。
    pub fn send_key(&mut self, virtual_key: u32, modifiers: KeyModifiers) -> (KeyOutcome, Option<String>, Option<String>, Vec<Candidate>) {
        if self.broken {
            return (KeyOutcome::Passthrough, None, None, Vec::new());
        }
        let sent = write_message(
            &mut self.stream,
            &ClientMessage::Key {
                session: self.session,
                event: KeyEvent {
                    virtual_key,
                    modifiers,
                },
            },
        );
        let reply = sent.and_then(|()| {
            let reply: Option<ServerMessage> = read_message(&mut self.stream)?;
            Ok(reply)
        });
        match reply {
            Ok(Some(ServerMessage::KeyResult { outcome, insert, preedit, candidates, .. })) => (outcome, insert, preedit, candidates),
            _ => {
                self.broken = true;
                (KeyOutcome::Passthrough, None, None, Vec::new())
            }
        }
    }
}

/// %LOCALAPPDATA%\Taiyang\server.port (Server 启动时写)。
fn read_port_file() -> io::Result<u16> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无 LOCALAPPDATA"))?;
    let path = PathBuf::from(base).join("Taiyang").join("server.port");
    let text = std::fs::read_to_string(path)?;
    text.trim()
        .parse::<u16>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "端口文件内容不是端口号"))
}