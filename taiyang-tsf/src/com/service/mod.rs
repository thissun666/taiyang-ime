//! TextService: 每线程一个, 实现 ITfTextInputProcessor (激活/停用) 与 ITfKeyEventSink (收键)。
//! M2: Activate 时连 Server 开会话; 按键经协议问 Server 吃不吃; 连不上全放行。

mod key_sink;
mod processor;

use std::cell::{Cell, RefCell};

use windows::Win32::UI::TextServices::{ITfKeyEventSink, ITfTextInputProcessor, ITfThreadMgr};
use windows::core::implement;

use crate::client::TsClient;

/// 一个 TSF 文本服务实例 (每线程一个)。
#[implement(ITfTextInputProcessor, ITfKeyEventSink)]
pub struct TextService {
    /// 激活时拿到的线程管理器, 停用时反注册用。
    pub(crate) thread_mgr: RefCell<Option<ITfThreadMgr>>,

    /// TSF 分配的 client id (激活传入的 tid)。
    pub(crate) client_id: Cell<u32>,

    /// 到 Server 的会话。Server 没跑则 None = 所有按键放行。
    pub(crate) client: RefCell<Option<TsClient>>,

    /// TestKeyDown 拿到的裁决, KeyDown 取用 (同一次按键只问一次 Server)。
    pub(crate) pending_eat: Cell<Option<bool>>,
}

impl TextService {
    #[allow(clippy::new_without_default)] // 有 lock_module 副作用
    pub fn new() -> Self {
        crate::com::lock_module();
        Self {
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            client: RefCell::new(None),
            pending_eat: Cell::new(None),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::com::unlock_module();
    }
}