//! ITfTextInputProcessor: 激活时挂击键 sink + 连 Server, 停用时全撤。

use windows::Win32::UI::TextServices::{
    ITfKeyEventSink, ITfKeystrokeMgr, ITfTextInputProcessor_Impl, ITfThreadMgr, TF_PRESERVEDKEY, TF_MOD_CONTROL, TF_MOD_SHIFT,
};
use windows::core::{IUnknownImpl, Interface, Ref, Result};

use super::TextService_Impl;
use crate::client::TsClient;
use crate::com::log::log;

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        let thread_mgr = ptim.ok()?.clone();
        let keystroke: ITfKeystrokeMgr = thread_mgr.cast()?;
        let sink: ITfKeyEventSink = self.to_interface();
        unsafe { keystroke.AdviseKeyEventSink(tid, &sink, true)? };
        // v18.1: 保留键注册 (用现成keystroke变量; 签名按注册表源码对版)
        let regs = [
            (super::key_sink::g_var_word(), TF_PRESERVEDKEY { uVKey: 0x31, uModifiers: TF_MOD_CONTROL }),
            (super::key_sink::g_var_kana(), TF_PRESERVEDKEY { uVKey: 0x32, uModifiers: TF_MOD_CONTROL }),
            (super::key_sink::g_pal(), TF_PRESERVEDKEY { uVKey: 0x33, uModifiers: TF_MOD_CONTROL | TF_MOD_SHIFT }),
            (super::key_sink::g_var_word2(), TF_PRESERVEDKEY { uVKey: 0x31, uModifiers: TF_MOD_CONTROL | TF_MOD_SHIFT }),
            (super::key_sink::g_var_kana2(), TF_PRESERVEDKEY { uVKey: 0x32, uModifiers: TF_MOD_CONTROL | TF_MOD_SHIFT }),
            (super::key_sink::g_kanamode(), TF_PRESERVEDKEY { uVKey: 0x34, uModifiers: TF_MOD_CONTROL | TF_MOD_SHIFT }),
            (super::key_sink::g_palclose(), TF_PRESERVEDKEY { uVKey: 0x35, uModifiers: TF_MOD_CONTROL | TF_MOD_SHIFT }), // v19.6
        ];
        for (g, k) in &regs {
            let r = unsafe { keystroke.PreserveKey(tid, g, k, &[]) };
            super::key_sink::log_pub(&format!("[p{}] 保留键注册: vk={:#x} ok={}", std::process::id(), k.uVKey, r.is_ok()));
        }
        self.client_id.set(tid);
        *self.thread_mgr.borrow_mut() = Some(thread_mgr);

        // M2: 连 Server 开会话。连不上 (Server 没跑) 保持 None -> 按键全放行。
        match TsClient::connect(u64::from(tid)) {
            Ok(client) => {
                log("已连接 Server, 会话开启");
                *self.client.borrow_mut() = Some(client);
            }
            Err(error) => log(&format!("Server 连不上 ({error}), 按键全放行")),
        }
        log(&format!("太阳 TSF v19.6 已激活 tid={tid}"));
        crate::com::service::key_sink::set_tid(tid);
        Ok(())
    }

    fn Deactivate(&self) -> Result<()> {
        // 先断会话再撤 sink, 顺序无所谓但都要做。
        if self.client.borrow_mut().take().is_some() {
            log("会话已断开");
        }
        if let Some(thread_mgr) = self.thread_mgr.borrow_mut().take()
            && let Ok(keystroke) = thread_mgr.cast::<ITfKeystrokeMgr>()
        {
            let _ = unsafe { keystroke.UnadviseKeyEventSink(self.client_id.get()) };
        }
        log("太阳 TSF 已停用");
        Ok(())
    }
}