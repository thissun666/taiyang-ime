//! ITfKeyEventSink: 按键 -> 问 Server -> 吃/放。
//! windows 0.62 生成接口: OnKeyDown/OnKeyUp 的 pfkeaten out 参数是 Result<BOOL> 返回值 (TRUE=吃)。

use taiyang_platform::protocol::{Candidate, KeyModifiers, KeyOutcome};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::TextServices::{ITfContext, ITfKeyEventSink_Impl};
use windows::core::{BOOL, GUID, Ref, Result};

use super::TextService_Impl;
use crate::com::log::log;
use windows::Win32::UI::TextServices::{
    ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, TF_AE_END,
    TF_ANCHOR_END, TF_ES_READWRITE, TF_SELECTION, TF_SELECTIONSTYLE, ITfComposition,
    ITfCompositionSink, ITfCompositionSink_Impl, ITfContextComposition, TF_IAS_QUERYONLY,

};

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        if fforeground.as_bool() {
            log("OnSetFocus: 前台");
        }
        Ok(())
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        unsafe { ensure_window(); } // v4: 预建窗避开编辑会话重入
        LAST_PIC.with(|p| *p.borrow_mut() = pic.ok().ok().cloned()); // v17: 调色盘直插上下文
        LAST_DOWN.with(|c| c.set(wparam.0 as u32)); // v15: Shift轻点=真实键序
        // v15: Enter 还给应用 (误发送防护): 弃组句+清缓冲, 键不吞 — 英文直输改 Shift 轻点
        if COMPOSITION.with(|c| c.borrow().is_some()) && wparam.0 as u32 == 0x0D {
            let _ = ask_server(self, 0x1B);
            if let Ok(ctx) = pic.ok() { do_edit(&ctx, None, None); }
            self.pending_eat.set(Some(false)); // v15.1: 只在此处设缓存 (KeyDown 将读取)
            return Ok(BOOL::from(false));
        }
        let (outcome, insert, preedit, _cands) = ask_server(self, wparam.0 as u32);
        if outcome == KeyOutcome::Consumed && preedit.as_deref().is_some_and(|p| !p.is_empty()) {
            log(&format!("[p{}] 首键显示: py={:?}", std::process::id(), preedit));
            unsafe { caret_fallback(); } // v5: 首键即现 (不依赖编辑会话排版; 随后 GetTextExt 精化)
        }
        let eaten = outcome == KeyOutcome::Consumed;
        self.pending_eat.set(Some(eaten));
        if let Ok(ctx) = pic.ok() {
            do_edit(&ctx, insert.as_deref(), preedit.as_deref());
        }
        Ok(BOOL::from(eaten))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        // 用 TestKeyDown 缓存的裁决; 万一缓存空了 (app 没先调 Test) 才现问。
        let eaten = match self.pending_eat.take() {
            Some(eaten) => eaten,
            None => {
                LAST_DOWN.with(|c| c.set(wparam.0 as u32));
                LAST_PIC.with(|p| *p.borrow_mut() = pic.ok().ok().cloned()); // v17: 调色盘直插上下文
                // v15: Enter 还给应用 — 仅 KeyDown-only 应用走到这里 (064: Win11记事本实证);
                // v15.1: 不设 pending_eat (已被 take, 本键即终结, 泄漏会吃掉下一键的 IME 路由)
                if COMPOSITION.with(|c| c.borrow().is_some()) && wparam.0 as u32 == 0x0D {
                    let _ = ask_server(self, 0x1B);
                    if let Ok(ctx) = pic.ok() { do_edit(&ctx, None, None); }
                    return Ok(BOOL::from(false));
                }
                let (outcome, insert, preedit, _cands) = ask_server(self, wparam.0 as u32);
                if outcome == KeyOutcome::Consumed && preedit.as_deref().is_some_and(|p| !p.is_empty()) {
                    log(&format!("[p{}] 首键显示: py={:?}", std::process::id(), preedit));
                    unsafe { caret_fallback(); } // v5: 首键即现
                }
                if let Ok(ctx) = pic.ok() {
                    do_edit(&ctx, insert.as_deref(), preedit.as_deref());
                }
                outcome == KeyOutcome::Consumed
            },
        };
        Ok(BOOL::from(eaten))
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        if wparam.0 as u32 == 0x10 {
            log(&format!("[p{}] TestKeyUp: Shift到货", std::process::id())); // v15.1: 到货诊断
        }
        Ok(BOOL(0)) // 不拦抬起
    }

    fn OnKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        // v15: Shift 轻点 = 原文直输 (中文=拼音字母 / 日语=假名; 用户拍板 Enter 还给应用)
        let vk = wparam.0 as u32;
        if matches!(vk, 0x10 | 0xA0 | 0xA1) {
            log(&format!("[p{}] KeyUp: Shift到货 last={:#x} py非空={}", std::process::id(),
                LAST_DOWN.with(|c| c.get()), PREEDIT.with(|c| !c.borrow().is_empty()))); // v15.1: 到货诊断
        }
        if matches!(vk, 0x10 | 0xA0 | 0xA1) && LAST_DOWN.with(|c| c.get()) == 0x10
            && PREEDIT.with(|c| !c.borrow().is_empty()) {
            LAST_DOWN.with(|c| c.set(0));
            let (o, ins, _, _) = ask_server(self, 0x0D); // 合成Enter → server 原文臂 (日语模式出假名)
            if o == KeyOutcome::Consumed {
                if let Some(text) = ins {
                    PREEDIT.with(|p| *p.borrow_mut() = String::new());
                    CANDS.with(|c| *c.borrow_mut() = Vec::new());
                    if let Ok(ctx) = pic.ok() { do_edit(&ctx, Some(&text), None); }
                    return Ok(BOOL::from(true));
                }
            }
        }
        Ok(BOOL(0))
    }
    fn OnPreservedKey(&self, pic: Ref<ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        let g = unsafe { *rguid }; // v18.1: 裸指针解引用入unsafe
        log(&format!("[p{}] 保留键进入", std::process::id())); // v19: 送达性探针
        // v18: 保留键 (系统级送达): Ctrl+1=日语词 Ctrl+2=假名 Ctrl+Shift+3=调色盘开关
        if g == g_palclose() {
            let open = PAL_HWND.with(|c| c.get()) != 0;
            if open { unsafe { pal_close(); } }
            log(&format!("[p{}] 调色盘: Ctrl+Shift+5 关闭(盘开={})", std::process::id(), open));
            return Ok(BOOL::from(true));
        }
        if g == g_kanamode() {
            let now = !KANA_MODE.with(|m| m.get());
            KANA_MODE.with(|m| m.set(now));
            log(&format!("[p{}] 假名键盘模式: {}", std::process::id(), if now { "开" } else { "关" }));
            return Ok(BOOL::from(true));
        }
        if g == g_pal() {
            log(&format!("[p{}] 保留键: 调色盘开关", std::process::id()));
            let open_now = PAL_HWND.with(|c| c.get()) == 0;
            if open_now {
            let _ = ask_server(self, 0x1B);
                if let Ok(ctx) = pic.ok() { LAST_PIC.with(|p| *p.borrow_mut() = Some(ctx.clone())); do_edit(ctx, None, None); } // v19.1: 开盘即抓上下文
                unsafe { pal_open(); }
            } else {
                unsafe { pal_close(); }
            }
            return Ok(BOOL::from(true));
        }
        if g == g_var_word() || g == g_var_kana() || g == g_var_word2() || g == g_var_kana2() {
            let want_kana = g == g_var_kana() || g == g_var_kana2();
            let is_v2 = g == g_var_word2() || g == g_var_kana2();
            let sel = SEL.with(|s| s.get()) as u32;
            let comp = COMPOSITION.with(|c| c.borrow().is_some());
            log(&format!("[p{}] 保留键: 变体 假名={} v{} 组句中={} sel={}", std::process::id(), want_kana, if is_v2 { 2 } else { 1 }, comp, sel)); // v19.4
            let sel = SEL.with(|s| s.get()) as u32;
            let comp = COMPOSITION.with(|c| c.borrow().is_some());
            let syn = if want_kana { 0x87 + sel } else { 0x97 + sel };
            let (o, ins, _p, _c) = ask_server(self, syn);
            let o_str = if o == KeyOutcome::Consumed { "Consumed" } else { "Other" };
            log(&format!("[p{}] 变体应答: syn={:#x} o={} ins_len={}", std::process::id(), syn, o_str, ins.as_ref().map(|s| s.len()).unwrap_or(0))); // v19.3: 应答探针
            if o == KeyOutcome::Consumed {
                if let Some(text) = ins {
                    PREEDIT.with(|p| *p.borrow_mut() = String::new());
                    CANDS.with(|c| *c.borrow_mut() = Vec::new());
                    if let Ok(ctx) = pic.ok() {
                        let ok = do_edit(ctx, Some(text.as_str()), None);
                        log(&format!("[p{}] 变体上屏: {text} ok={ok}", std::process::id()));
                    } else {
                        log(&format!("[p{}] 变体: pic不可用", std::process::id()));
                    }
                    return Ok(BOOL::from(true));
                }
            }
            return Ok(BOOL::from(false)); // 无组句缓冲 → 不吃, 还给应用
        }
        Ok(BOOL(0))
    }
}

/// 问 Server 一次。无会话 = 放行; 有会话交给 client (失败也在那里转放行)。
fn ask_server(sink: &TextService_Impl, virtual_key: u32) -> (KeyOutcome, Option<String>, Option<String>, Vec<Candidate>) {
    // v19.5: syn 合成键直通道 — 根因修复: v10a(ctrl臂)/v10b(白名单)把递归 syn 键吃在 send_key 之前,
    // 变体协议(v16)从出生就没到过 server (G1b 铁证: server 侧 main.rs L166 早已实现). 零修饰防让位臂.
    // 排除 0xA0..=0xA5 (VK_L/R SHIFT/CTRL/MENU 真修饰键); 词变体 sel 9-14 暂牺牲 (0xA0-0xA5 撞 VK 区)
    if (0x87..=0x9F).contains(&virtual_key) || virtual_key == 0xA6 {
        let mut zm = modifiers_now();
        zm.ctrl = false; zm.alt = false; zm.shift = false; zm.win = false;
        let sent = {
            let mut cg = sink.client.borrow_mut();
            match cg.as_mut() {
                Some(cl) => Some(cl.send_key(virtual_key, zm)),
                None => None,
            }
        };
        if let Some((outcome, insert, preedit, cands)) = sent {
            PREEDIT.with(|p| *p.borrow_mut() = preedit.clone().unwrap_or_default());
            CANDS.with(|c| *c.borrow_mut() = cands.clone());
            log(&format!("[p{}] syn直通: vk={:#x} o={} ins_len={}", std::process::id(), virtual_key, if outcome == KeyOutcome::Consumed { 1 } else { 0 }, insert.as_ref().map(|s| s.len()).unwrap_or(0)));
            return (outcome, insert, preedit, cands);
        }
        return (KeyOutcome::Passthrough, None, None, Vec::new());
    }
    // v19.2: JIS 假名键盘模式 (Ctrl+Shift+4 开关) — 命中返回 Consumed+insert, 交给 handler 提交
    let caps_on = (unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x14) } & 1) != 0; // v19.2: ask_server 无 caps 参数, 本地算 (0x14=VK_CAPITAL)
    if KANA_MODE.with(|m| m.get()) && !caps_on {
        if virtual_key == 0x1B {
            KANA_MODE.with(|m| m.set(false));
            log(&format!("[p{}] 假名键盘模式: 关(Esc)", std::process::id()));
            return (KeyOutcome::Consumed, None, None, Vec::new());
        }
        if let Some(k) = jis_map(virtual_key) {
            log(&format!("[p{}] 假名键盘模式: {k}", std::process::id()));
            return (KeyOutcome::Consumed, Some(k.to_string()), None, Vec::new());
        }
    }
    // v5: 组句中 ←/→ 翻页, ↑/↓ 移动高亮, 空格提交高亮项 (DLL 本地消费, 不进 server 防让位误上屏)
    let comp_alive = COMPOSITION.with(|c| c.borrow().is_some());
    let total = CANDS.with(|c| c.borrow().len());
    let mods = modifiers_now();
    // v16.2: Ctrl/Alt+1/2 = 日语变体提交 (Ctrl 必达备胎: Alt 走 WM_SYSKEYDOWN 多数应用不送 IME; Alt 臂另保留) + 到货探针
    if (mods.ctrl || mods.alt) && (0x31..=0x32).contains(&virtual_key) {
        log(&format!("[p{}] 组合键探针: vk={:#x} ctrl={} alt={} 组句中={}", std::process::id(), virtual_key, mods.ctrl, mods.alt, comp_alive));
        if comp_alive {
            let sel = SEL.with(|s| s.get()) as u32;
            let comp = COMPOSITION.with(|c| c.borrow().is_some());
            let syn = if virtual_key == 0x31 { 0x97 + sel } else { 0x87 + sel };
            let (o, ins, _p, _c) = ask_server(sink, syn);
            return (o, ins, None, Vec::new());
        }
    }
    // v13: Shift+字母 = 英文直输 (用户点名回归的老需求; 组句中先净场再放行大写)
    let caps_on = (unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x14) } as u16 & 1) != 0; // v14: CapsLock开=大写英文直输
    if (mods.shift || caps_on) && (0x41..=0x5A).contains(&virtual_key) {
        if comp_alive {
            let _ = ask_server(sink, 0x1B);
        }
        return (KeyOutcome::Passthrough, None, None, Vec::new());
    }    // v16.1: Alt+1/2 = 日语变体提交 (1=日语词 2=假名; 裸数字仍中文; Shift+数字=原生标点!@#; 合成键0x97+/0x87+)
    if comp_alive && mods.alt && (0x31..=0x32).contains(&virtual_key) {
        let sel = SEL.with(|s| s.get()) as u32;
        let syn = if virtual_key == 0x31 { 0x97 + sel } else { 0x87 + sel };
        let (o, ins, _p, _c) = ask_server(sink, syn);
        return (o, ins, None, Vec::new());
    }

    // v16.1: PrtSc 截图净场 (清组句+透传 — 截图后 dou 弹窗残留案)
    if comp_alive && virtual_key == 0x2C {
        let _ = ask_server(sink, 0x1B);
        return (KeyOutcome::Passthrough, None, None, Vec::new());
    }    // v17.1: 反引号(0xC0) = 调色盘全局开关 (组句内外都开; Caps=英文模式逃生照打反引号)
    if virtual_key == 0xC0 && !caps_on {
        let open_now = PAL_HWND.with(|c| c.get()) == 0;
        if open_now {
            let _ = ask_server(sink, 0x1B);
            unsafe { pal_open(); }
        } else {
            unsafe { pal_close(); }
        }
        return (KeyOutcome::Consumed, None, None, Vec::new());
    }    // v15: 中文标点 (组句中): 提交当前候选(有候选)/原文(无候选) + 全角标点; Shift/Caps/组合键=英文标点放行
    if comp_alive && !mods.shift && !caps_on && !mods.ctrl && !mods.alt && !mods.win
        && matches!(virtual_key, 0xBA | 0xBC | 0xBE | 0xBF | 0xC0 | 0xDB | 0xDC | 0xDD | 0xDE) {
        let punct = match virtual_key {
            0xBC => "，", 0xBE => "。", 0xBF => "、", 0xBA => "；",
            0xDB => "【", 0xDD => "】", 0xDC => "、", 0xC0 => "·",
            _ => { if QUOTE_OPEN.with(|c| c.get()) { QUOTE_OPEN.with(|c| c.set(false)); "”" } else { QUOTE_OPEN.with(|c| c.set(true)); "“" } }
        };
        let committed = if total > 0 {
            ask_server(sink, 0x31 + SEL.with(|s| s.get()) as u32).1
        } else {
            ask_server(sink, 0x0D).1 // 无候选: 原文直输 + 标点
        };
        return (KeyOutcome::Consumed, committed.map(|x| format!("{x}{punct}")), None, Vec::new());
    }
    // v8: 纯修饰键永不进 server (server 让位臂见怪键就提交拼音 —— "一按 Ctrl 就开复制"之根); 放行+组句原样

    if comp_alive && total > 0 {
        match virtual_key {
            // v8: 组句中 Ctrl+V = 粘贴净场: Esc 清 server 缓冲 + 空组句收尾 (拼音不落文本), 键放行给粘贴

            0x25 | 0x27 => {
                let maxp = (total - 1) / PAGE_PER;
                let cur = PAGE.with(|p| p.get());
                let np = if virtual_key == 0x25 { cur.saturating_sub(1) } else { (cur + 1).min(maxp) };
                PAGE.with(|p| p.set(np));
                SEL.with(|s| s.set(np * PAGE_PER));
                let py = PREEDIT.with(|c| c.borrow().clone());
                return (KeyOutcome::Consumed, None, Some(py), Vec::new());
            }
            0x26 | 0x28 => {
                let cur = SEL.with(|s| s.get());
                let ns = if virtual_key == 0x26 { cur.saturating_sub(1) } else { (cur + 1).min(total - 1) };
                SEL.with(|s| s.set(ns));
                PAGE.with(|p| p.set(ns / PAGE_PER));
                let py = PREEDIT.with(|c| c.borrow().clone());
                return (KeyOutcome::Consumed, None, Some(py), Vec::new());
            }
            0x20 if SEL.with(|s| s.get()) > 0 => {
                return ask_server(sink, 0x31 + SEL.with(|s| s.get()) as u32);
            }
            _ => {}
        }
    }
    // v10a: Ctrl/Alt/Win 组合键不进 server (复制/粘贴等快捷键; server 让位臂见怪键就提交拼音) — Esc 例外 (净场自用)
    if comp_alive && (mods.ctrl || mods.alt || mods.win) && !(mods.ctrl && virtual_key == 0x1B) {
        // Ctrl+V 粘贴净场: Esc 清 server 缓冲 + 空组句收尾, 拼音不落文本
        if mods.ctrl && virtual_key == 0x56 {
            let _ = ask_server(sink, 0x1B);
            PREEDIT.with(|p| *p.borrow_mut() = String::new());
            CANDS.with(|c| *c.borrow_mut() = Vec::new());
            return (KeyOutcome::Passthrough, None, None, Vec::new()); // v12: Esc式彻底终组句(空preedit=僵尸空组句会吞粘贴文本)
        }
        let py = PREEDIT.with(|c| c.borrow().clone());
        return (KeyOutcome::Passthrough, None, Some(py), Vec::new());
    }
    // v10b: 白名单反演 — 组句中只送 server 认识的键 (字母/数字选词/退格/回车/Esc/空格);
    // 其余一切 (PrtSc/F键/Insert/方向/翻页/修饰...) 本地放行且组句原样 — 截图粘贴落拼音的总根
    if comp_alive && !matches!(virtual_key, 0x41..=0x5A | 0x30..=0x39 | 0x08 | 0x0D | 0x1B | 0x20 | 0x70 | 0x71 | 0x72 | 0x73) { // v15: F1-F4 全量(F2总开关/F1F3F4日 子模式): +F2 日语切换(078功能, 088白名单误杀)
        let py = PREEDIT.with(|c| c.borrow().clone());
        return (KeyOutcome::Passthrough, None, Some(py), Vec::new());
    }
    let mut client = sink.client.borrow_mut();
    let r = match client.as_mut() {
        Some(client) => {
            if STALE.with(|s| s.get()) {
                let _ = client.send_key(0x1B, modifiers_now()); // 先清强杀残留的 server 缓冲
                STALE.with(|s| s.set(false));
            }
            let mods = modifiers_now();
    // v16.2: Ctrl/Alt+1/2 = 日语变体提交 (Ctrl 必达备胎: Alt 走 WM_SYSKEYDOWN 多数应用不送 IME; Alt 臂另保留) + 到货探针
    if (mods.ctrl || mods.alt) && (0x31..=0x32).contains(&virtual_key) {
        log(&format!("[p{}] 组合键探针: vk={:#x} ctrl={} alt={} 组句中={}", std::process::id(), virtual_key, mods.ctrl, mods.alt, comp_alive));
        if comp_alive {
            let sel = SEL.with(|s| s.get()) as u32;
            let comp = COMPOSITION.with(|c| c.borrow().is_some());
            let syn = if virtual_key == 0x31 { 0x97 + sel } else { 0x87 + sel };
            let (o, ins, _p, _c) = ask_server(sink, syn);
            return (o, ins, None, Vec::new());
        }
    }
    // v13: Shift+字母 = 英文直输 (用户点名回归的老需求; 组句中先净场再放行大写)
    let caps_on = (unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x14) } as u16 & 1) != 0; // v14: CapsLock开=大写英文直输
    if (mods.shift || caps_on) && (0x41..=0x5A).contains(&virtual_key) {
        if comp_alive {
            let _ = ask_server(sink, 0x1B);
        }
        return (KeyOutcome::Passthrough, None, None, Vec::new());
    }    // v16.1: Alt+1/2 = 日语变体提交 (1=日语词 2=假名; 裸数字仍中文; Shift+数字=原生标点!@#; 合成键0x97+/0x87+)
    if comp_alive && mods.alt && (0x31..=0x32).contains(&virtual_key) {
        let sel = SEL.with(|s| s.get()) as u32;
        let syn = if virtual_key == 0x31 { 0x97 + sel } else { 0x87 + sel };
        let (o, ins, _p, _c) = ask_server(sink, syn);
        return (o, ins, None, Vec::new());
    }

    // v16.1: PrtSc 截图净场 (清组句+透传 — 截图后 dou 弹窗残留案)
    if comp_alive && virtual_key == 0x2C {
        let _ = ask_server(sink, 0x1B);
        return (KeyOutcome::Passthrough, None, None, Vec::new());
    }    // v17.1: 反引号(0xC0) = 调色盘全局开关 (组句内外都开; Caps=英文模式逃生照打反引号)
    if virtual_key == 0xC0 && !caps_on {
        let open_now = PAL_HWND.with(|c| c.get()) == 0;
        if open_now {
            let _ = ask_server(sink, 0x1B);
            unsafe { pal_open(); }
        } else {
            unsafe { pal_close(); }
        }
        return (KeyOutcome::Consumed, None, None, Vec::new());
    }    // v15: 中文标点 (组句中): 提交当前候选(有候选)/原文(无候选) + 全角标点; Shift/Caps/组合键=英文标点放行
    if comp_alive && !mods.shift && !caps_on && !mods.ctrl && !mods.alt && !mods.win
        && matches!(virtual_key, 0xBA | 0xBC | 0xBE | 0xBF | 0xC0 | 0xDB | 0xDC | 0xDD | 0xDE) {
        let punct = match virtual_key {
            0xBC => "，", 0xBE => "。", 0xBF => "、", 0xBA => "；",
            0xDB => "【", 0xDD => "】", 0xDC => "、", 0xC0 => "·",
            _ => { if QUOTE_OPEN.with(|c| c.get()) { QUOTE_OPEN.with(|c| c.set(false)); "”" } else { QUOTE_OPEN.with(|c| c.set(true)); "“" } }
        };
        let committed = if total > 0 {
            ask_server(sink, 0x31 + SEL.with(|s| s.get()) as u32).1
        } else {
            ask_server(sink, 0x0D).1 // 无候选: 原文直输 + 标点
        };
        return (KeyOutcome::Consumed, committed.map(|x| format!("{x}{punct}")), None, Vec::new());
    }
            let no_mod = !(mods.ctrl || mods.alt || mods.win || mods.shift);
            let (outcome, insert, preedit, cands) = client.send_key(virtual_key, mods);
            // v6 侦探: 无修饰裸字母 server 必吃; 若 Passthrough = 僵尸连接 (server 半路被停)
            let dead = outcome == KeyOutcome::Passthrough
                && (0x41..=0x5A).contains(&virtual_key)
                && no_mod;
            if dead {
                log(&format!("[p{}] v6 侦探: 僵尸连接, 弃旧重连", std::process::id()));
                no_server_fallback();
                None
            } else {
                PREEDIT.with(|p| *p.borrow_mut() = preedit.clone().unwrap_or_default());
                CANDS.with(|c| *c.borrow_mut() = cands.clone());
                PAGE.with(|p| p.set(0));
                SEL.with(|s| s.set(0));
            log(&format!("[p{}] 键应: vk={:#x} 候选={} py={:?}", std::process::id(), virtual_key, cands.len(), preedit));
                Some((outcome, insert, preedit, cands))
            }
        }
        None => {
            no_server_fallback();
            None
        }
    };
    drop(client);
    match r {
        Some(r) => r,
        None => {
            // v5: server 刚拉起 -> 立即重连并补发本键 (否则本进程永远直通英文)
            if let Ok(c) = crate::client::TsClient::connect(TID.with(|t| t.get()) as u64) {
                *sink.client.borrow_mut() = Some(c);
                let mut client = sink.client.borrow_mut();
                if let Some(cl) = client.as_mut() {
                    let (outcome, insert, preedit, cands) = cl.send_key(virtual_key, modifiers_now());
                    PREEDIT.with(|p| *p.borrow_mut() = preedit.clone().unwrap_or_default());
                    CANDS.with(|c| *c.borrow_mut() = cands.clone());
                    PAGE.with(|p| p.set(0));
                    SEL.with(|s| s.set(0));
            log(&format!("[p{}] 键应: vk={:#x} 候选={} py={:?}", std::process::id(), virtual_key, cands.len(), preedit));
                    log(&format!("[p{}] v5 已重连 server 并接管", std::process::id()));
                    return (outcome, insert, preedit, cands);
                }
            }
            (KeyOutcome::Passthrough, None, None, Vec::new())
        }
    }
}
/// 当前物理修饰键状态。如实上报: 否则 Ctrl+C 会被当字母 c 吃掉。
fn modifiers_now() -> KeyModifiers {
    let down = |vk: i32| unsafe { GetKeyState(vk) } as u16 & 0x8000 != 0;
    KeyModifiers {
        ctrl: down(VK_CONTROL.0 as i32),
        shift: down(VK_SHIFT.0 as i32),
        alt: down(VK_MENU.0 as i32),
        win: down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32),
    }
}

// ===== M3 Part 2: 插字 (ITfEditSession + ITfInsertAtSelection) =====

thread_local! {
    /// Activate 时记录的 client id (RequestEditSession 需要)。
    static TID: std::cell::Cell<u32> = std::cell::Cell::new(0);
}

/// Activate 实现里注入调用, 把 tid 存进本线程。
pub fn set_tid(tid: u32) {
    TID.with(|t| t.set(tid));
}

/// 光标移到插入末尾: 否则下一次插入仍落在原选区, 连续上屏的字从右往左堆 (青简 verbatim)。
fn move_selection_to_end(context: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let end = unsafe { range.Clone()? };
    unsafe { end.Collapse(ec, TF_ANCHOR_END)? };
    let selection = TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(end)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_END,
            fInterimChar: false.into(),
        },
    };
    // SetSelection 不接管 range 的所有权, 之后手动释放 (青简注释)。
    let result = unsafe { context.SetSelection(ec, std::slice::from_ref(&selection)) };
    drop(std::mem::ManuallyDrop::into_inner(selection.range));
    result
}

// ===== M4: 组句 (composition) — 预编辑内联显示 =====

thread_local! {
    /// 当前活动组句; None = 无组句。
    static COMPOSITION: std::cell::RefCell<Option<ITfComposition>> = std::cell::RefCell::new(None);
}

fn has_composition() -> bool {
    COMPOSITION.with(|c| c.borrow().is_some())
}

#[windows::core::implement(ITfCompositionSink)]
struct CompSink;

impl ITfCompositionSink_Impl for CompSink_Impl {
    /// 应用强行终止组句 (切焦点等): 拼音已沦为普通文本, 丢指针即可。
    fn OnCompositionTerminated(&self, ecwrite: u32, pcomposition: windows::core::Ref<ITfComposition>) -> Result<()> {
        log("组句被应用终止");

        // v9: 强杀组句时 TSF 把拼音留成普通文本 (截图/切焦点落拼音之根) — 组句范围 SetText 空串删之。
        // 守卫: PREEDIT 为纯拼音才删 (正常上屏后 PREEDIT 已空 + 范围是汉字, 双保险绝不误删)
        if let Ok(comp) = pcomposition.ok() {
            if let Ok(range) = unsafe { comp.GetRange() } {
                let py = PREEDIT.with(|c| c.borrow().clone());
                if !py.is_empty() && py.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
                    match unsafe { range.SetText(ecwrite, 0, &[]) } {
                        Ok(_) => {
                            PREEDIT.with(|p| *p.borrow_mut() = String::new());
                            CANDS.with(|c| *c.borrow_mut() = Vec::new());
                            log(&format!("[p{}] 已删除落下拼音: {}", std::process::id(), py));
                        }
                        Err(e) => log(&format!("[p{}] 拼音删除失败({}): {}", std::process::id(), py, e)),
                    }
                }
            }
        }        COMPOSITION.with(|c| *c.borrow_mut() = None);
        STALE.with(|s| s.set(true)); // 组句被应用强杀: server 残缓冲待清 (防拼音复制)
        hide_window();
        Ok(())
    }
}

/// 在当前选区起一个空组句 (青简姿势: QUERYONLY 拿 range, 不插字)。
fn start_composition(ctx: &ITfContext, ec: u32) -> Result<ITfComposition> {
    let insert: ITfInsertAtSelection = windows::core::Interface::cast(ctx)?;
    let range = unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? };
    let cc: ITfContextComposition = windows::core::Interface::cast(ctx)?;
    let sink: ITfCompositionSink = CompSink.into();
    let composition = unsafe { cc.StartComposition(ec, &range, &sink)? };
    COMPOSITION.with(|c| *c.borrow_mut() = Some(composition.clone()));
    log("组句已开始");
    Ok(composition)
}

/// 把组句文本设为 preedit (无组句则先起一个), 光标跟进到组句尾。
fn set_preedit(ctx: &ITfContext, ec: u32, text: &[u16]) -> Result<()> {
    let composition = match COMPOSITION.with(|c| c.borrow().clone()) {
        Some(c) => c,
        None => start_composition(ctx, ec)?,
    };
    let range = unsafe { composition.GetRange()? };
    unsafe { range.SetText(ec, 0, text)? };
    move_selection_to_end(ctx, ec, &range)?;
    Ok(())
}

/// 结束组句: 先清空文本再 End, 防残留拼音 (青简注释)。
fn end_composition(ec: u32) -> Result<()> {
    if let Some(composition) = COMPOSITION.with(|c| c.borrow_mut().take()) {
        let range = unsafe { composition.GetRange()? };
        unsafe { range.SetText(ec, 0, &[])? };
        unsafe { composition.EndComposition(ec)? };
        log("组句已结束");
    }
    Ok(())
}

/// M4 统一编辑会话: commit(上屏) / preedit(组句更新) / 清组句, 三选一。
#[windows::core::implement(ITfEditSession)]
struct M4Session {
    ctx: ITfContext,
    commit: Option<Vec<u16>>,
    preedit: Option<Vec<u16>>,
}

impl ITfEditSession_Impl for M4Session_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        if let Some(text) = &self.commit {
            // 上屏: 有组句 = 原地替换成候选再结束 (青简 apply); 无组句 = M3 插字兜底
            let composition = COMPOSITION.with(|c| c.borrow_mut().take());
            let range = match composition {
                Some(composition) => {
                    let range = unsafe { composition.GetRange()? };
                    unsafe { range.SetText(ec, 0, text)? };
                    unsafe { composition.EndComposition(ec)? };
                    log("编辑会话: 组句已上屏");
                    range
                }
                None => {
                    let ias: ITfInsertAtSelection = windows::core::Interface::cast(&self.ctx)?;
                    let range = unsafe {
                        ias.InsertTextAtSelection(ec, windows::Win32::UI::TextServices::INSERT_TEXT_AT_SELECTION_FLAGS(0), text)?
                    };
                    log("编辑会话: 插入完成");
                    range
                }
            };
            hide_window();
            move_selection_to_end(&self.ctx, ec, &range)?;
            log("编辑会话: 光标已跟进");
            return Ok(());
        }
        if let Some(text) = &self.preedit {
            set_preedit(&self.ctx, ec, text)?;
            place_at_caret(&self.ctx, ec);
            log("编辑会话: 组句已更新");
            return Ok(());
        }
        hide_window();
        end_composition(ec)?; // 缓冲被清 (Esc 等): 静默收尾
        Ok(())
    }
}

/// 异步提交 M4 编辑会话。三态: 上屏 / 组句 / 清组句; 全空且无组句 = 无事可做。
fn do_edit(ctx: &ITfContext, commit: Option<&str>, preedit: Option<&str>) -> bool {
    if commit.is_none() && preedit.is_none() && !has_composition() {
        return true;
    }
    let session: ITfEditSession = M4Session {
        ctx: ctx.clone(),
        commit: commit.map(|t| t.encode_utf16().collect()),
        preedit: preedit.map(|t| t.encode_utf16().collect()),
    }
    .into();
    let tid = TID.with(|t| t.get());
    match unsafe { ctx.RequestEditSession(tid, &session, TF_ES_READWRITE) } {
        Ok(_) => true,
        Err(e) => {
            log(&format!("编辑会话请求失败: {e}"));
            false
        }
    }
}
// ===== M5: 学习候选窗 (typer v0.7 灵魂移植; ☆生词标待M6生词本迁server) =====

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LRESULT, RECT};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, ClientToScreen, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DeleteObject, FW_NORMAL, GetDC, HDC, HFONT,
    HGDIOBJ, InvalidateRect, OUT_DEFAULT_PRECIS, PatBlt, PATCOPY, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, ValidateRect, SetWindowRgn,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetGUIThreadInfo, GetSystemMetrics, GUITHREADINFO, GUITHREADINFO_FLAGS, RegisterClassExW,
    SetWindowPos, ShowWindow, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_PAINT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const CAND_CLASS: PCWSTR = w!("TaiyangCandWnd");
const C_BG: u32 = 0x00FFFFFF;
const C_BORDER: u32 = 0x00E0E0E0;
const C_PY: u32 = 0x00666666;
const C_CARET: u32 = 0x00888888;
const C_TXT: u32 = 0x002F2F2F;
const C_SELBG: u32 = 0x00FCF0E0;
const C_SELTXT: u32 = 0x002B2B2B;
const C_GLOSS: u32 = 0x00A6937C;
const C_SELGLOSS: u32 = 0x00D9904A;
const C_NUM: u32 = 0x00D9904A;

thread_local! {
    static CAND_WND: std::cell::Cell<usize> = std::cell::Cell::new(0);
    static CANDS: std::cell::RefCell<Vec<Candidate>> = std::cell::RefCell::new(Vec::new());
    static PREEDIT: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static CUR_WH: std::cell::Cell<(i32, i32)> = std::cell::Cell::new((480, 120));
    static CLASS_OK: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static FONTS: std::cell::Cell<(isize, isize, u32)> = std::cell::Cell::new((0, 0, 96));
    static DPI: std::cell::Cell<u32> = std::cell::Cell::new(96);
}

fn est_w(s: &str, cjk: i32, asc: i32) -> i32 {
    s.chars().map(|c| if c as u32 > 0x2E80 { cjk } else { asc }).sum()
}

fn create_font(h: i32) -> HFONT {
    unsafe {
        CreateFontW(h, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
            0, w!("Microsoft YaHei"))
    }
}

/// DPI 缩放: 96 基准。
fn dp(x: i32) -> i32 {
    let d = DPI.with(|d| d.get()) as i64;
    ((x as i64 * d) / 96) as i32
}

fn fonts() -> (HFONT, HFONT) {
    FONTS.with(|f| {
        let d = DPI.with(|d| d.get());
        let (a, b, vd) = f.get();
        if a == 0 || vd != d {
            if a != 0 {
                unsafe {
                    let _ = DeleteObject(std::mem::transmute::<isize, HFONT>(a).into());
                    let _ = DeleteObject(std::mem::transmute::<isize, HFONT>(b).into());
                }
            }
            let big = create_font(-((30 * d as i32) / 96));
            let sma = create_font(-((18 * d as i32) / 96));
            f.set((big.0 as isize, sma.0 as isize, d));
            (big, sma)
        } else {
            unsafe { (std::mem::transmute::<isize, HFONT>(a), std::mem::transmute::<isize, HFONT>(b)) }
        }
    })
}
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn ensure_window() {
    if CAND_WND.with(|c| c.get()) != 0 { return; }
    if !CLASS_OK.with(|c| c.get()) {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(cand_wndproc),
            lpszClassName: CAND_CLASS,
            hInstance: dll_hinst(),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        CLASS_OK.with(|c| c.set(true));
    }
    if let Ok(hwnd) = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        CAND_CLASS, w!(""), WS_POPUP,
        0, 0, 600, 160, None, None,
        Some(dll_hinst()), None,
    ) {
        CAND_WND.with(|c| c.set(hwnd.0 as usize));
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        DPI.with(|d| d.set(if dpi == 0 { 96 } else { dpi }));
        log(&format!("[p{}] M5 候选窗 v19.6 就绪 dpi={dpi}", std::process::id()));
    }
}

pub fn hide_window() {
    let wid = CAND_WND.with(|c| c.get());
    if wid != 0 {
        unsafe { let _ = ShowWindow(HWND(wid as *mut core::ffi::c_void), SW_HIDE); }
    }
}

/// 窗随键 v4: 每页 4 条 (左右键翻页), DPI 感知, 宽 540-720。
fn show_candidates(x: i32, y: i32) {
    let (cands, _, _) = page_slice();
    log(&format!("[p{}] 显示: 候选={} 拼音={}", std::process::id(), cands.len(), PREEDIT.with(|c| c.borrow().clone())));
    if PREEDIT.with(|c| c.borrow().is_empty()) && cands.is_empty() { hide_window(); return; } // v9: 有拼音必弹窗(kk 无词也出, 对齐其他输入法)
    unsafe { ensure_window(); }
    unsafe {
        let wid = CAND_WND.with(|c| c.get());
        if wid == 0 { return; }
        let dpi = GetDpiForWindow(HWND(wid as *mut core::ffi::c_void));
        DPI.with(|d| d.set(if dpi == 0 { 96 } else { dpi }));
    }
    let mut w = if cands.is_empty() { dp(300) } else { dp(540) };
    for c in &cands {
        let g = c.gloss.clone().unwrap_or_default();
        w = w.max(dp(46) + est_w(&c.text, dp(30), dp(16)) + dp(16) + est_w(&g, dp(18), dp(9)) + dp(56));
    }
    w = w.min(dp(720));
    let h = dp(48) + cands.len().max(if PREEDIT.with(|c| c.borrow().is_empty()) { 0 } else { 1 }) as i32 * dp(44) + dp(14);
    CUR_WH.with(|c| c.set((w, h)));
    unsafe {
        let wid = CAND_WND.with(|c| c.get());
        let hwnd = HWND(wid as *mut core::ffi::c_void);
        let mut x = x.max(0);
        let mut y = y;
        if x + w > GetSystemMetrics(SM_CXSCREEN) - 8 { x = GetSystemMetrics(SM_CXSCREEN) - 8 - w; }
        if y + h > GetSystemMetrics(SM_CYSCREEN) - 8 { y = (y - h - 30).max(0); }
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, dp(12), dp(12));
        let _ = SetWindowRgn(hwnd, Some(rgn), true);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
thread_local! {
    /// 最后一次有效光标屏幕位 (v8: GetTextExt 吐垃圾矩形时粘住, 弹窗不再跳走/消失)。
    static LAST_CARET: std::cell::Cell<(i32, i32)> = std::cell::Cell::new((0, 0));
    static HAD_CARET: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

/// 组句定位 v8: GetTextExt 左下; 零宽矩形 (left==right 但有高度) 是组句首键的合法光标位 (tsf.log 实证 (542,1222,542,1260)); 垃圾矩形粘上一光标位; 无历史退鼠标。
fn place_at_caret(ctx: &ITfContext, ec: u32) {
    let mut ok = false;
    if let Some(composition) = COMPOSITION.with(|c| c.borrow().clone()) {
        if let Ok(range) = unsafe { composition.GetRange() } {
            if let Ok(view) = unsafe { ctx.GetActiveView() } {
                let mut rc = RECT::default();
                let mut clipped = BOOL(0);
                let _ = unsafe { view.GetTextExt(ec, &range, &mut rc, &mut clipped) };
                log(&format!("[p{}] 定位: GetTextExt=({},{},{},{}) clipped={}", std::process::id(), rc.left, rc.top, rc.right, rc.bottom, clipped.0));
                if rc.bottom > rc.top && rc.left >= 0 {
                    let y = rc.bottom + 6;
                    LAST_CARET.with(|c| c.set((rc.left, y)));
                    HAD_CARET.with(|c| c.set(true));
                    show_candidates(rc.left, y);
                    ok = true;
                }
            }
        }
    }
    if !ok {
        let (lx, ly) = LAST_CARET.with(|c| c.get());
        if HAD_CARET.with(|c| c.get()) {
            log(&format!("[p{}] 定位: 垃圾矩形, 粘上一光标位", std::process::id()));
            show_candidates(lx, ly);
        } else {
            log(&format!("[p{}] 定位: GetTextExt 不可用, 走兜底", std::process::id()));
            unsafe { caret_fallback(); }
        }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn caret_fallback() {
    let mut gti = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        flags: GUITHREADINFO_FLAGS(0),
        hwndActive: HWND::default(), hwndFocus: HWND::default(),
        hwndCapture: HWND::default(), hwndMenuOwner: HWND::default(),
        hwndMoveSize: HWND::default(), hwndCaret: HWND::default(),
        rcCaret: RECT::default(),
    };
    if GetGUIThreadInfo(0, &mut gti).is_ok() && !gti.hwndCaret.0.is_null() {
        let mut p = POINT { x: gti.rcCaret.left, y: gti.rcCaret.bottom };
        let _ = ClientToScreen(gti.hwndCaret, &mut p);
        LAST_CARET.with(|c| c.set((p.x, p.y + 2)));
        HAD_CARET.with(|c| c.set(true));
        log(&format!("[p{}] 定位: 系统光标", std::process::id()));
        show_candidates(p.x, p.y + 2);
        return;
    }
    let (lx, ly) = LAST_CARET.with(|c| c.get());
    if HAD_CARET.with(|c| c.get()) {
        log(&format!("[p{}] 定位: 粘上一光标位", std::process::id()));
        show_candidates(lx, ly);
        return;
    }
    let mut c = POINT::default();
    let _ = GetCursorPos(&mut c);
    log(&format!("[p{}] 定位: 鼠标兜底", std::process::id()));
    show_candidates(c.x + 6, c.y + 8);
}

#[allow(unsafe_op_in_unsafe_fn)]unsafe extern "system" fn cand_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        paint_candidates(hwnd);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn paint_candidates(hwnd: HWND) {
    let (w, h) = CUR_WH.with(|c| c.get());
    let (cands, start, total) = page_slice();
    log(&format!("[p{}] 绘制: 本页={} 总={} 高亮={}", std::process::id(), cands.len(), total, SEL.with(|s| s.get())));
    let py = PREEDIT.with(|c| c.borrow().clone());
    let hdc = GetDC(Some(hwnd));
    fill(hdc, 0, 0, w, h, C_BG);
    SetBkMode(hdc, TRANSPARENT);
    fill(hdc, 0, 0, w, 1, C_BORDER);
    fill(hdc, 0, h - 1, w, 1, C_BORDER);
    fill(hdc, 0, 0, 1, h, C_BORDER);
    fill(hdc, w - 1, 0, 1, h, C_BORDER);
    let (fbig, fsma) = fonts();
    SelectObject(hdc, HGDIOBJ(fsma.0));
    text(hdc, dp(18), dp(13), &py, C_PY);
    fill(hdc, dp(18) + est_w(&py, dp(18), dp(9)) + dp(4), dp(14), 1, dp(22), C_CARET);
    if cands.is_empty() {
        SelectObject(hdc, HGDIOBJ(fsma.0));
        text(hdc, dp(20), dp(48) + dp(9), "无匹配 · 继续输入或 Esc 清除", C_PY);
        ReleaseDC(Some(hwnd), hdc);
        let _ = ValidateRect(Some(hwnd), None);
        return;
    }
    if total > 0 {
        let ind = format!("{}-{}/{}  ←→翻页 ↑↓选词", start + 1, start + cands.len(), total);
        let iw = est_w(&ind, dp(18), dp(9));
        text(hdc, w - dp(18) - iw, dp(13), &ind, C_PY);
    }
    for (ri, c) in cands.iter().enumerate() {
        let ry = dp(48) + ri as i32 * dp(44);
        let sel = start + ri == SEL.with(|s| s.get());
        if sel { fill(hdc, dp(8), ry - dp(2), w - dp(16), dp(40), C_SELBG); }
        SelectObject(hdc, HGDIOBJ(fsma.0));
        text(hdc, dp(20), ry + dp(9), &(start + ri + 1).to_string(), C_NUM);
        SelectObject(hdc, HGDIOBJ(fbig.0));
        text(hdc, dp(46), ry + dp(2), &c.text, if sel { C_SELTXT } else { C_TXT });
        if let Some(gl) = &c.gloss {
            SelectObject(hdc, HGDIOBJ(fsma.0));
            let gx = dp(46) + est_w(&c.text, dp(30), dp(16)) + dp(16);
            let badge = if c.level > 0 { format!(" ·L{}", c.level) } else { String::new() };
            let mut g = gl.clone();
            while gx + est_w(&format!("{g}{badge}"), dp(18), dp(9)) > w - dp(18) && g.chars().count() > 2 { g.pop(); }
            g.push_str(&badge);
            text(hdc, gx, ry + dp(9), &g, if sel { C_SELGLOSS } else { C_GLOSS });
        }
    }
    ReleaseDC(Some(hwnd), hdc);
    let _ = ValidateRect(Some(hwnd), None);
}
fn fill(hdc: HDC, x: i32, y: i32, w: i32, h: i32, color: u32) {
    unsafe {
        let hb = CreateSolidBrush(COLORREF(color));
        let old = SelectObject(hdc, HGDIOBJ(hb.0));
        let _ = PatBlt(hdc, x, y, w, h, PATCOPY);
        SelectObject(hdc, old);
        let _ = DeleteObject(hb.into());
    }
}

fn text(hdc: HDC, x: i32, y: i32, s: &str, color: u32) {
    unsafe {
        let _ = SetTextColor(hdc, COLORREF(color));
        let u: Vec<u16> = s.encode_utf16().collect();
        let _ = unsafe { TextOutW(hdc, x, y, &u) }; // v19.2: 灭 E0133 警告
    }
}

fn dll_hinst() -> HINSTANCE {
    unsafe { HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0) }
}

// M5 候选窗补充导入 (0.62 源码定位)
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::POINT;

thread_local! {
    /// 组句被应用强杀后, server 残缓冲待清 (qingjian server_stale 同款)。
    static STALE: std::cell::Cell<bool> = std::cell::Cell::new(false);
}
use windows::Win32::UI::HiDpi::GetDpiForWindow;

// ===== v4: 翻页 + server 就地拉起 =====
const PAGE_PER: usize = 4;

thread_local! {
    /// 候选页码 (每页 PAGE_PER 条, ←→ 翻页)。
    static PAGE: std::cell::Cell<usize> = std::cell::Cell::new(0);
}

static SPAWN_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 当前页可见候选 + 绝对起始下标 + 总数 (序号绝对化, server 数字选词零改动)。
fn page_slice() -> (Vec<Candidate>, usize, usize) {
    let all = CANDS.with(|c| c.borrow().clone());
    let total = all.len();
    if total == 0 { return (Vec::new(), 0, 0); }
    let maxp = (total - 1) / PAGE_PER;
    let p = PAGE.with(|p| p.get()).min(maxp);
    let s = p * PAGE_PER;
    let e = (s + PAGE_PER).min(total);
    (all[s..e].to_vec(), s, total)
}

thread_local! {
    /// 当前高亮候选的绝对下标 (↑/↓ 移动, 空格提交, 新候选到达归零)。
    static SEL: std::cell::Cell<usize> = std::cell::Cell::new(0);
}

/// 无 server: 后台静默拉起 (CREATE_NO_WINDOW 不弹黑窗; 日志重定向 server.log); 10s 冷却可重试。
fn no_server_fallback() -> (KeyOutcome, Option<String>, Option<String>, Vec<Candidate>) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    if now.saturating_sub(SPAWN_AT.load(std::sync::atomic::Ordering::SeqCst)) > 10_000 {
        SPAWN_AT.store(now, std::sync::atomic::Ordering::SeqCst);
        let exe = std::env::var("TAIYANG_SERVER").ok().map(std::path::PathBuf::from)
            .or_else(|| Some(std::path::PathBuf::from(r"E:\GitHubtwo\myinput\rust\target\release\taiyang-server.exe")))
            .filter(|p| p.exists());
        if let Some(exe) = exe {
            use std::os::windows::process::CommandExt;
            let dir = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Taiyang");
            let _ = std::fs::create_dir_all(&dir);
            let lg = dir.join("server.log");
            let mut cmd = std::process::Command::new(&exe);
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 输入法拉起服务不该弹黑窗
            cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()); // v7: null 兜底——宿主(chrome等)无控制台, 日志打开失败时 server println! 会 panic 秒退(0字节之谜)
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&lg) {
                if let Ok(f2) = f.try_clone() {
                    cmd.stdout(std::process::Stdio::from(f));
                    cmd.stderr(std::process::Stdio::from(f2));
                }
            }
            let ok = cmd.spawn().is_ok();
            // v13: 异步拉起 (同步600ms曾挂在首键/切换线程上=慢切换之根); 冷启动窗口诚实落一键, 侦探随后自愈
            log(&format!("[p{}] server 未运行: 后台拉起{} (日志 {})", std::process::id(), if ok { "成功" } else { "失败" }, lg.display()));
        } else {
            log(&format!("[p{}] server 未运行: 未找到 server.exe", std::process::id()));
        }
    }
    (KeyOutcome::Passthrough, None, None, Vec::new())
}
// v15: Shift 轻点检测 + 引号开合 (DLL 线程仓)
thread_local! {
    static LAST_DOWN: std::cell::Cell<u32> = const { std::cell::Cell::new(0u32) };
    static QUOTE_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}


// ===== v19: 五十音图调色盘 v2 (加大+美化+平/片切换+DPI自适应; 开关=Ctrl+Shift+3 或反引号) =====
pub fn g_var_word() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f01) }
pub fn g_var_kana() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f02) }
pub fn g_pal() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f03) }
pub fn g_var_word2() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f04) }
pub fn g_var_kana2() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f05) }
pub fn g_kanamode() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f06) }

pub fn g_palclose() -> windows::core::GUID { windows::core::GUID::from_u128(0x6f1e2a30_5c4b_4d8e_9a7f_1b3c5d7e9f07) } // v19.6: Ctrl+Shift+5 关盘
pub fn log_pub(s: &str) { log(s); }
// v19.1: JIS 假名键盘模式键位表 (图2方案; Ctrl+Shift+4 开关)
pub fn jis_map(vk: u32) -> Option<&'static str> {
    let k: &str = match vk {
        0x30 => "わ", 0x31 => "ぬ", 0x32 => "ふ", 0x33 => "あ", 0x34 => "う",
        0x35 => "え", 0x36 => "お", 0x37 => "や", 0x38 => "ゆ", 0x39 => "よ",
        0x51 => "た", 0x57 => "て", 0x45 => "い", 0x52 => "す", 0x54 => "か",
        0x59 => "ん", 0x55 => "な", 0x49 => "に", 0x4F => "ら", 0x50 => "せ",
        0x41 => "ち", 0x53 => "と", 0x44 => "し", 0x46 => "は", 0x47 => "き",
        0x48 => "く", 0x4A => "ま", 0x4B => "の", 0x4C => "り",
        0x5A => "つ", 0x58 => "さ", 0x43 => "そ", 0x56 => "ひ", 0x42 => "こ",
        0x4E => "み", 0x4D => "も",
        0xBA => "れ", 0xBB => "ー", 0xBC => "ね", 0xBD => "ほ", 0xBE => "る",
        0xBF => "め", 0xC0 => "ろ", 0xDB => "」", 0xDC => "・", 0xDD => "む",
        0xDE => "け", 0xE2 => "・",
        _ => return None,
    };
    Some(k)
}

static KANA_HIRA: &[&str] = &[
    "あ","い","う","え","お","が","ぎ","ぐ","げ","ご",
    "か","き","く","け","こ","ざ","じ","ず","ぜ","ぞ",
    "さ","し","す","せ","そ","だ","ぢ","づ","で","ど",
    "た","ち","つ","て","と","ば","び","ぶ","べ","ぼ",
    "な","に","ぬ","ね","の","ぱ","ぴ","ぷ","ぺ","ぽ",
    "は","ひ","ふ","へ","ほ","ま","み","む","め","も",
    "や","ゆ","よ","ら","り","る","れ","ろ","わ","を",
    "ん","っ","ぁ","ぃ","ぅ","ぇ","ぉ","ゃ","ゅ","ょ",
    "ー","「","」","。","、","・","×","×","×","×",
    "きゃ","きゅ","きょ","しゃ","しゅ","しょ","ちゃ","ちゅ","ちょ","にゃ",
    "にゅ","にょ","ひゃ","ひゅ","ひょ","みゃ","みゅ","みょ","りゃ","りゅ",
    "りょ","ぎゃ","ぎゅ","ぎょ","じゃ","じゅ","じょ","びゃ","びゅ","びょ",
    "ぴゃ","ぴゅ","ぴょ","ふぁ","ふぃ","ふぇ","ふぉ","ゔ","×","×",
];
static KANA_KATA: &[&str] = &[
    "ア","イ","ウ","エ","オ","ガ","ギ","グ","ゲ","ゴ",
    "カ","キ","ク","ケ","コ","ザ","ジ","ズ","ゼ","ゾ",
    "サ","シ","ス","セ","ソ","ダ","ヂ","ヅ","デ","ド",
    "タ","チ","ツ","テ","ト","バ","ビ","ブ","ベ","ボ",
    "ナ","ニ","ヌ","ネ","ノ","パ","ピ","プ","ペ","ポ",
    "ハ","ヒ","フ","ヘ","ホ","マ","ミ","ム","メ","モ",
    "ヤ","ユ","ヨ","ラ","リ","ル","レ","ロ","ワ","ヲ",
    "ン","ヴ","ッ","ャ","ュ","ョ","ー","×","×","×",
    "「","」","。","、","・","×","×","×","×","×",
];
const KANA_COLS: i32 = 10;
const HIRA_ROWS: i32 = 13;
const KATA_ROWS: i32 = 9;
const CELL: i32 = 54;
const HEADER: i32 = 46;
static WCLASS: &[u16] = &[0x54,0x61,0x69,0x79,0x61,0x6E,0x67,0x4B,0x61,0x6E,0x61,0];
static WTITLE: &[u16] = &[0x47,0x6F,0x6A,0x75,0x6F,0x6E,0x20,0x76,0x31,0x39,0]; // Gojuon v19

thread_local! {
    static LAST_PIC: std::cell::RefCell<Option<ITfContext>> = const { std::cell::RefCell::new(None) };
    static PAL_HWND: std::cell::Cell<isize> = const { std::cell::Cell::new(0isize) };
    static PAL_PAGE: std::cell::Cell<u8> = const { std::cell::Cell::new(0u8) };
    static PAL_FONT: std::cell::Cell<isize> = const { std::cell::Cell::new(0isize) };
    static PAL_CELL: std::cell::Cell<i32> = const { std::cell::Cell::new(CELL) };
    static KANA_MODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PAL_FLASH: std::cell::Cell<i32> = const { std::cell::Cell::new(-1) };
    static PAL_LAST: std::cell::Cell<i32> = const { std::cell::Cell::new(-1) };
}

unsafe fn pal_text(hdc: windows::Win32::Graphics::Gdi::HDC, s: &str, x: i32, y: i32) {
    use windows::Win32::Graphics::Gdi::*;
    let mut u: Vec<u16> = s.encode_utf16().collect();
    let _ = unsafe { TextOutW(hdc, x, y, &u) }; // v19.2: 灭 E0133 警告
}

// v19.3: 矩形居中文本 (DrawTextW; 修页头切半/格内24px硬编码偏移两丑源)
unsafe fn pal_text_c(hdc: windows::Win32::Graphics::Gdi::HDC, s: &str, rc: windows::Win32::Foundation::RECT) {
    let mut u: Vec<u16> = s.encode_utf16().collect();
    let mut rc = rc;
    let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut u, &mut rc, windows::Win32::Graphics::Gdi::DT_CENTER | windows::Win32::Graphics::Gdi::DT_VCENTER | windows::Win32::Graphics::Gdi::DT_SINGLELINE);
}

fn pal_insert(k: &str) {
    log(&format!("[p{}] 调色盘点击: {k}", std::process::id()));
    let ctx = LAST_PIC.with(|p| p.borrow().clone());
    if let Some(c) = ctx {
        let ok = do_edit(&c, Some(k), None);
        log(&format!("[p{}] 调色盘写入: {k} ok={ok}", std::process::id()));
    } else {
        log(&format!("[p{}] 调色盘无上下文(LAST_PIC空) {k}", std::process::id()));
    }
}

unsafe fn pal_close() {
    let hw = PAL_HWND.with(|c| c.get());
    if hw != 0 {
        PAL_HWND.with(|c| c.set(0));
        let f = PAL_FONT.with(|c| c.get());
        if f != 0 {
            PAL_FONT.with(|c| c.set(0));
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(f as *mut core::ffi::c_void));
        }
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(HWND(hw as _));
    }
}

unsafe extern "system" fn pal_proc(hw: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    match msg {
        WM_ERASEBKGND => { return LRESULT(1); } // v19.6: 灭背景擦除闪

        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hw, &mut ps);
            let cell = PAL_CELL.with(|c| c.get());
            let page = PAL_PAGE.with(|c| c.get());
            let rows = if page == 0 { HIRA_ROWS } else { KATA_ROWS };
            let cw = KANA_COLS * cell;
            let s = (GetDpiForWindow(hw).max(96) / 96) as i32; let chh = HEADER * s + HIRA_ROWS * cell;
            let bg = CreateSolidBrush(COLORREF(0x00F8F6F2));
            let wh = CreateSolidBrush(COLORREF(0x00FFFFFF));
            let bl = CreateSolidBrush(COLORREF(0x004084F0));
            let rd = CreateSolidBrush(COLORREF(0x004A58E8));
            let ln = CreateSolidBrush(COLORREF(0x00DCD8D4));
            let tn = CreateSolidBrush(COLORREF(0x00D0E8FF)); // v19.4: 余痕浅橙
            let rc_all = RECT { left: 0, top: 0, right: cw, bottom: chh };
            // v19.6: 双缓冲 — 绘制全入内存DC (灭点击逐格重画闪=闪缩根之二)
            let hdc_real = hdc;
            let mdc = CreateCompatibleDC(Some(hdc));
            let bmp = CreateCompatibleBitmap(hdc, cw, chh);
            let oldb = SelectObject(mdc, HGDIOBJ(bmp.0));
            let hdc = mdc;
            let _ = FillRect(hdc, &rc_all, bg);
            let b1 = RECT { left: 8 * s, top: 6 * s, right: 88 * s, bottom: HEADER * s - 6 * s };
            let b2 = RECT { left: 96 * s, top: 6 * s, right: 176 * s, bottom: HEADER * s - 6 * s };
            let b3 = RECT { left: cw - 84 * s, top: 6 * s, right: cw - 8 * s, bottom: HEADER * s - 6 * s };
            let _ = FillRect(hdc, &b1, if page == 0 { bl } else { wh });
            let _ = FillRect(hdc, &b2, if page == 1 { bl } else { wh });
            let _ = FillRect(hdc, &b3, rd);
            let _ = FrameRect(hdc, &b1, ln);
            let _ = FrameRect(hdc, &b2, ln);
            let _ = FrameRect(hdc, &b3, ln);
            let fi = PAL_FONT.with(|c| c.get());
            let oldf = SelectObject(hdc, if fi != 0 { HGDIOBJ(fi as *mut core::ffi::c_void) } else { GetStockObject(DEFAULT_GUI_FONT) });
            let _ = SetBkMode(hdc, TRANSPARENT); // v19.3: 文字底透明
            let _ = SetBkColor(hdc, if page == 0 { COLORREF(0x004084F0) } else { COLORREF(0x00FFFFFF) });
            let _ = SetTextColor(hdc, if page == 0 { COLORREF(0x00FFFFFF) } else { COLORREF(0x00302A28) });
            pal_text_c(hdc, "あ", b1);
            let _ = SetBkColor(hdc, if page == 1 { COLORREF(0x004084F0) } else { COLORREF(0x00FFFFFF) });
            let _ = SetTextColor(hdc, if page == 1 { COLORREF(0x00FFFFFF) } else { COLORREF(0x00302A28) });
            pal_text_c(hdc, "ア", b2);
            let _ = SetBkColor(hdc, COLORREF(0x004A58E8));
            let _ = SetTextColor(hdc, COLORREF(0x00FFFFFF));
            pal_text_c(hdc, "閉", b3);
            let _ = SetBkColor(hdc, COLORREF(0x00FFFFFF));
            let _ = SetTextColor(hdc, COLORREF(0x00302A28));
            let grid: &[&str] = if page == 0 { KANA_HIRA } else { KANA_KATA };
            for r in 0..rows {
                for c in 0..KANA_COLS {
                    let x = c * cell; let y = HEADER * s + r * cell; // v19.4: 网格y补缩放
                    let rc = RECT { left: x, top: y, right: x + cell, bottom: y + cell };
                    let idx = r * KANA_COLS + c;
                    let hot = PAL_FLASH.with(|f| f.get()) == idx && grid[idx as usize] != "×";
                    let lastc = PAL_LAST.with(|l| l.get()) == idx && grid[idx as usize] != "×";
                    let _ = FillRect(hdc, &rc, if hot { bl } else if lastc { tn } else { wh });
                    let _ = FrameRect(hdc, &rc, ln);
                    let k = grid[(r * KANA_COLS + c) as usize];
                    if k != "×" {
                        let n = k.encode_utf16().count() as i32;
                        let _ = SetTextColor(hdc, if PAL_FLASH.with(|f| f.get()) == r * KANA_COLS + c { COLORREF(0x00FFFFFF) } else { COLORREF(0x00302A28) });
                        pal_text_c(hdc, k, rc);
                    }
                }
            }
            let _ = SelectObject(hdc, oldf);
            let _ = DeleteObject(HGDIOBJ(bg.0));
            let _ = DeleteObject(HGDIOBJ(wh.0));
            let _ = DeleteObject(HGDIOBJ(bl.0));
            let _ = DeleteObject(HGDIOBJ(rd.0));
            let _ = DeleteObject(HGDIOBJ(ln.0));
            let _ = DeleteObject(HGDIOBJ(tn.0));
            let _ = BitBlt(hdc_real, 0, 0, cw, chh, Some(mdc), 0, 0, SRCCOPY); // v19.6: 一次上屏
            let _ = SelectObject(mdc, oldb);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mdc);
            let _ = EndPaint(hw, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let s = (GetDpiForWindow(hw).max(96) / 96) as i32;
            let cell = PAL_CELL.with(|c| c.get());
            let page = PAL_PAGE.with(|c| c.get());
            let rows = if page == 0 { HIRA_ROWS } else { KATA_ROWS };
            let x = (lp.0 & 0xFFFF) as u16 as i32;
            let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i32;
            if y >= HEADER * s {
                let r = (y - HEADER * s) / cell;
                let c = x / cell;
                if r >= 0 && r < rows && c >= 0 && c < KANA_COLS {
                    let grid: &[&str] = if page == 0 { KANA_HIRA } else { KANA_KATA };
                    if grid[(r * KANA_COLS + c) as usize] != "×" {
                        PAL_FLASH.with(|f| f.set(r * KANA_COLS + c));
                        let _ = InvalidateRect(Some(hw), None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => { let s = (GetDpiForWindow(hw).max(96) / 96) as i32;
            PAL_FLASH.with(|f| f.set(-1)); // v19.4: 抬起熄灭
            let cell = PAL_CELL.with(|c| c.get());
            let page = PAL_PAGE.with(|c| c.get());
            let rows = if page == 0 { HIRA_ROWS } else { KATA_ROWS };
            let x = (lp.0 & 0xFFFF) as u16 as i32;
            let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i32;
            if y < HEADER * s {
                if x < 92 * s { PAL_PAGE.with(|c| c.set(0)); let _ = InvalidateRect(Some(hw), None, true); return LRESULT(0); }
                if x < 184 * s { PAL_PAGE.with(|c| c.set(1)); let _ = InvalidateRect(Some(hw), None, true); return LRESULT(0); }
                let cw = KANA_COLS * cell;
                if x >= cw - 84 * s { pal_close(); }
                return LRESULT(0);
            }
            let r = (y - HEADER * s) / cell;
            let c = x / cell;
            if r >= 0 && r < rows && c >= 0 && c < KANA_COLS {
                let grid: &[&str] = if page == 0 { KANA_HIRA } else { KANA_KATA };
                let k = grid[(r * KANA_COLS + c) as usize];
                if k != "×" {
                    PAL_LAST.with(|l| l.set(r * KANA_COLS + c));
                    pal_insert(k);
                    let _ = InvalidateRect(Some(hw), None, false);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => { pal_close(); LRESULT(0) }
        WM_DESTROY => { PAL_HWND.with(|c| c.set(0)); LRESULT(0) }
        _ => DefWindowProcW(hw, msg, wp, lp),
    }
}

unsafe fn pal_open() {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;
    if PAL_HWND.with(|c| c.get()) != 0 { return; }
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| unsafe {
        let inst = GetModuleHandleW(windows::core::PCWSTR::null()).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(pal_proc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            lpszClassName: windows::core::PCWSTR::from_raw(WCLASS.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
    });
    PAL_PAGE.with(|c| c.set(0));
    if let Ok(hw) = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        windows::core::PCWSTR::from_raw(WCLASS.as_ptr()),
        windows::core::PCWSTR::from_raw(WTITLE.as_ptr()),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        120, 120, 400, 300,
        None, None, Some(GetModuleHandleW(windows::core::PCWSTR::null()).unwrap_or_default().into()), None,
    ) {
        let dpi = GetDpiForWindow(hw).max(96);
        let cell = CELL * dpi as i32 / 96;
        let head = HEADER * dpi as i32 / 96;
        PAL_CELL.with(|c| c.set(cell));
        let mut rc = RECT { left: 0, top: 0, right: KANA_COLS * cell, bottom: head + HIRA_ROWS * cell };
        windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut rc, WS_POPUP | WS_CAPTION | WS_SYSMENU, false, WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, dpi);
        let _ = SetWindowPos(hw, None, 120, 120, rc.right - rc.left, rc.bottom - rc.top, SWP_NOZORDER | SWP_NOACTIVATE);
        let fpx = (32 * dpi as i32 / 96).max(26);
        let face: Vec<u16> = "Yu Gothic UI\0".encode_utf16().collect();
        let fnt = CreateFontW(-fpx, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0u32, windows::core::PCWSTR::from_raw(face.as_ptr()));
        PAL_FONT.with(|c| c.set(fnt.0 as isize));
        let _ = ShowWindow(hw, SW_SHOW);
        PAL_HWND.with(|c| c.set(hw.0 as isize));
        log(&format!("[p{}] 调色盘打开: dpi={dpi} cell={cell}", std::process::id()));
    }
}