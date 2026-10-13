//! 太阳输入法 v0.7 - qingjian 式竖排候选: 序号+词+英文释义/选中浅蓝高亮/拼音行光标
use myinput_core::Engine;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::core::{w, Error as WinError, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, ClientToScreen, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
    DeleteObject, FW_NORMAL, GetDC, HDC, HFONT, HGDIOBJ, HRGN, InvalidateRect,
    OUT_DEFAULT_PRECIS, PATCOPY, PatBlt, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, SetWindowRgn, TextOutW, TRANSPARENT, ValidateRect,
};
use windows::Win32::System::Console::SetConsoleOutputCP;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetGUIThreadInfo,
    GetSystemMetrics, GUITHREADINFO, GUITHREADINFO_FLAGS, HCURSOR, HHOOK,
    KBDLLHOOKSTRUCT, KBDLLHOOKSTRUCT_FLAGS, LoadCursorW, MSG, PostQuitMessage,
    RegisterClassW, SetWindowPos, SetWindowsHookExW, ShowWindow, SM_CXSCREEN,
    SM_CYSCREEN, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, TranslateMessage,
    WH_KEYBOARD_LL, WM_DESTROY, WM_HOTKEY, WM_PAINT, WNDCLASSW, WNDCLASS_STYLES,
    HWND_TOPMOST, CS_DROPSHADOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

trait HwndOut { fn out(self) -> HWND; }
impl HwndOut for HWND { fn out(self) -> HWND { self } }
impl HwndOut for Result<HWND, WinError> { fn out(self) -> HWND { self.expect("候选窗创建失败") } }
trait HdcOut { fn out(self) -> HDC; }
impl HdcOut for HDC { fn out(self) -> HDC { self } }
impl HdcOut for Result<HDC, WinError> { fn out(self) -> HDC { self.expect("GetDC失败") } }
trait HookOut { fn out(self) -> HHOOK; }
impl HookOut for HHOOK { fn out(self) -> HHOOK { self } }
impl HookOut for Result<HHOOK, WinError> { fn out(self) -> HHOOK { self.expect("钩子安装失败") } }
trait RgnOut { fn out(self) -> HRGN; }
impl RgnOut for HRGN { fn out(self) -> HRGN { self } }
impl RgnOut for Result<HRGN, WinError> { fn out(self) -> HRGN { self.expect("rgn失败") } }
trait CurOut { fn out(self) -> HCURSOR; }
impl CurOut for HCURSOR { fn out(self) -> HCURSOR { self } }
impl CurOut for Result<HCURSOR, WinError> { fn out(self) -> HCURSOR { self.expect("cursor失败") } }
trait FontOut { fn out(self) -> HFONT; }
impl FontOut for HFONT { fn out(self) -> HFONT { self } }
impl FontOut for Result<HFONT, WinError> { fn out(self) -> HFONT { self.expect("font失败") } }

#[derive(Clone)]
struct Cand { text: String, w: i32, gloss: String }
static ENGINE: OnceLock<Engine> = OnceLock::new();
static COMPOSING: AtomicBool = AtomicBool::new(false);
static BUF: Mutex<String> = Mutex::new(String::new());
static CANDS: Mutex<Vec<Cand>> = Mutex::new(Vec::new());
static SEL: AtomicUsize = AtomicUsize::new(0);
static PAGE: AtomicUsize = AtomicUsize::new(0);
const PAGE_SIZE: usize = 3;
static CAND_WND: AtomicIsize = AtomicIsize::new(0);
static CUR_W: AtomicI32 = AtomicI32::new(320);
static CUR_H: AtomicI32 = AtomicI32::new(34);
static FGN: AtomicIsize = AtomicIsize::new(0);
static FONT_BIG: OnceLock<isize> = OnceLock::new();
static FONT_SMALL: OnceLock<isize> = OnceLock::new();
static JA_GLOSS: OnceLock<HashMap<String, String>> = OnceLock::new();
static JA_MODE: AtomicBool = AtomicBool::new(false);
static EN_MODE: AtomicBool = AtomicBool::new(false);
static SHIFT_DN: AtomicBool = AtomicBool::new(false);
static SHIFT_OT: AtomicBool = AtomicBool::new(false);
static VOCAB: OnceLock<Mutex<HashMap<String, (u32, u32)>>> = OnceLock::new();
static VOCAB_PATH: OnceLock<PathBuf> = OnceLock::new();
const LLKHF_INJECTED: KBDLLHOOKSTRUCT_FLAGS = KBDLLHOOKSTRUCT_FLAGS(0x10);
const WND_W: i32 = 360;
const IDC_ARROW_VAL: usize = 32512;
// qingjian 配色
const C_BG: u32 = 0x00FFFFFF;      // 白底
const C_BORDER: u32 = 0x00E0E0E0;  // 1px 浅灰边
const C_PY: u32 = 0x00666666;      // 拼音行
const C_CARET: u32 = 0x00888888;   // 拼音光标
const C_SELBG: u32 = 0x00FCF0E0;   // 选中行浅蓝 RGB(224,240,252)
const C_NUM: u32 = 0x00D9904A;     // 序号蓝 RGB(74,144,217)
const C_TXT: u32 = 0x002F2F2F;     // 候选词
const C_SELTXT: u32 = 0x002B2B2B;  // 选中词
const C_GLOSS: u32 = 0x00A6937C;   // 释义灰蓝 RGB(124,147,166)
const C_SELGLOSS: u32 = 0x00D9904A;// 选中释义蓝
const C_NEW: u32 = 0x00008CFF;     // 生词橙 RGB(255,140,0), 字节序0x00BBGGRR(P0探针反推)

fn est_w(s: &str, cjk: i32, asc: i32) -> i32 {
    s.chars().map(|c| if c as u32 >= 0x2E80 { cjk } else { asc }).sum()
}

// 释义清洗: 竖线->空格 / 分号->间隔点 / 压缩空白
fn fmt_gloss(s: &str) -> String {
    let t = s.replace('|', " ").replace(';', " · ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

// 日语格式: 汉字|假名 -> 汉字（假名）; 纯假名原样
fn fmt_ja(s: &str) -> String {
    match s.split_once('|') {
        Some((w, k)) => format!("{}（{}）", w.trim(), k.trim()),
        None => s.trim().to_string(),
    }
}

// 释义查询(053): 整词 -> 逐字兜底; en单词条取前2义防长串
fn gloss_of(e: &Engine, ja_on: bool, wd: &str) -> String {
    let clean = |s: &str| -> String {
        let g = if ja_on { fmt_ja(s) } else { fmt_gloss(s) };
        if !ja_on {
            let ps: Vec<&str> = g.split(" · ").take(2).collect();
            return ps.join(" · ");
        }
        g
    };
    let raw = if ja_on {
        JA_GLOSS.get().and_then(|m| m.get(wd)).cloned()
    } else {
        e.gloss(wd).map(|s| s.to_string())
    };
    if let Some(s) = raw {
        let g = clean(&s);
        if !g.is_empty() { return g; }
    }
    if wd.chars().count() > 1 {
        let mut pcs: Vec<String> = Vec::new();
        for c in wd.chars() {
            let cs = c.to_string();
            let r = if ja_on {
                JA_GLOSS.get().and_then(|m| m.get(cs.as_str())).cloned()
            } else {
                e.gloss(cs.as_str()).map(|s| s.to_string())
            };
            if let Some(s) = r {
                // 逐字兜底每字只取第一义(054): 压总长防人名类省略号
                let g0 = if ja_on { fmt_ja(&s) } else { fmt_gloss(&s) };
                let g = if ja_on { g0 } else {
                    g0.split(" · ").next().unwrap_or("").trim().to_string()
                };
                if !g.is_empty() && !pcs.contains(&g) { pcs.push(g); }
            }
        }
        return pcs.join(if ja_on { " " } else { " · " });
    }
    String::new()
}

// 生词本(055): seen/commit 双计数(抄青简 record_exposure/commit 语义)
fn vocab_get(w: &str) -> (u32, u32) {
    VOCAB.get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.get(w).cloned())
        .unwrap_or((0, 0))
}
fn vocab_note(w: &str, cmt: bool) {
    if let Some(m) = VOCAB.get() {
        if let Ok(mut g) = m.lock() {
            let e = g.entry(w.to_string()).or_insert((0u32, 0u32));
            e.0 += 1;
            if cmt { e.1 += 1; }
        }
    }
}
fn vocab_save() {
    if let (Some(m), Some(p)) = (VOCAB.get(), VOCAB_PATH.get()) {
        if let Ok(g) = m.lock() {
            let mut ws: Vec<(&String, &(u32, u32))> = g.iter().collect();
            ws.sort();
            let mut out = vec!["# myinput 生词本 v1\tword\tseen\tcommit".to_string()];
            for (w, (s, c)) in ws { out.push(format!("{}\t{}\t{}", w, s, c)); }
            let _ = fs::write(p, out.join("\n"));
        }
    }
}
// 等级桶(055): freq(zipf*1000)分五级
fn freq_lv(e: &Engine, w: &str) -> i32 {
    match e.freq(w) {
        Some(f) if f >= 3000.0 => 1,
        Some(f) if f >= 1500.0 => 2,
        Some(f) if f >= 600.0 => 3,
        Some(f) if f >= 200.0 => 4,
        Some(_) => 5,
        None => 0,
    }
}

fn create_font(h: i32) -> HFONT {
    unsafe {
        CreateFontW(h, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0,
            DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32, w!("Microsoft YaHei")).out()
    }
}

fn mods_now() -> u32 {
    let mut m = 0u32;
    unsafe {
        if GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 { m |= 1; }
        if GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 { m |= 2; }
        if GetAsyncKeyState(VK_MENU.0 as i32) < 0 { m |= 4; }
        if GetAsyncKeyState(VK_LWIN.0 as i32) < 0 { m |= 8; }
    }
    m
}

fn send_unicode(s: &str) {
    let mut inputs: Vec<INPUT> = Vec::new();
    for u in s.encode_utf16() {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            inputs.push(INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: Default::default(), wScan: u,
                    dwFlags: flags, time: 0, dwExtraInfo: 0 } } });
        }
    }
    if !inputs.is_empty() {
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32); }
    }
}

fn buf_empty() -> bool { BUF.lock().unwrap().is_empty() }
fn cands_len() -> usize { CANDS.lock().unwrap().len() }

fn handle_key(vk: u32) -> bool {
    let m = mods_now();
    if m & 6 == 6 && vk == 0x4D { return false; }
    // Ctrl+数字 = 页内候选译文上屏(054, 抄青简0.1.0)
    if m & 2 != 0 && m & 4 == 0 && (0x31u32..=0x35).contains(&vk) {
        let cands = CANDS.lock().unwrap();
        if cands.is_empty() { return false; }
        let start = PAGE.load(Ordering::Relaxed) * PAGE_SIZE;
        let cnt = (cands.len() - start).min(PAGE_SIZE);
        let idx = (vk - 0x31) as usize;
        if idx >= cnt { return false; }
        let g = cands[start + idx].gloss.trim_end_matches('…').to_string();
        drop(cands);
        if g.is_empty() { return false; }
        send_unicode(&g);
        println!("  [太阳] 译文上屏: {}", g);
        BUF.lock().unwrap().clear();
        ui_refresh();
        return true;
    }
    if m != 0 { return false; }
    match vk {
        0x41..=0x5A => { append((vk as u8 as char).to_ascii_lowercase()); ui_refresh(); true }
        0xDE        => { append('\''); ui_refresh(); true }
        0x08        => {
            let popped = BUF.lock().unwrap().pop().is_some();
            if popped { ui_refresh(); true } else { false }
        }
        0x20        => {
            if cands_len() > 0 { commit_sel(SEL.load(Ordering::Relaxed)); true }
            else if !buf_empty() { commit_raw(); true }
            else { false }
        }
        v @ 0x31..=0x39 => {
            if cands_len() == 0 { false } else {
                let start = PAGE.load(Ordering::Relaxed) * PAGE_SIZE;
                let cnt = (cands_len() - start).min(PAGE_SIZE);
                let idx = (v - 0x31) as usize;
                if idx >= cnt { true } else { commit_sel(start + idx); true }
            }
        }
        0x0D        => { if buf_empty() { false } else { commit_raw(); true } }
        0x70        => { set_en(true); true }
        0x71        => { let was = JA_MODE.fetch_xor(true, Ordering::Relaxed); println!("  [太阳] 释义: {}", if was { "英语" } else { "日语" }); ui_refresh(); true }
        0x72        => {
            let cands = CANDS.lock().unwrap();
            if cands.is_empty() { return false; }
            let sel = SEL.load(Ordering::Relaxed).min(cands.len() - 1);
            let w = cands[sel].text.clone();
            drop(cands);
            vocab_note(&w, true);
            vocab_save();
            println!("  [F3] 认识: {}", w);
            ui_refresh();
            true
        }
        0x1B        => { compose_off(); true }
        0x25        => { if cands_len() > 1 { page_flip(-1); true } else { false } }
        0x26        => { if cands_len() > 0 { move_sel(-1); true } else { false } }
        0x27        => { if cands_len() > 1 { page_flip(1); true } else { false } }
        0x28        => { if cands_len() > 0 { move_sel(1); true } else { false } }
        _ => false,
    }
}

fn append(c: char) {
    let mut b = BUF.lock().unwrap();
    if b.chars().count() < 40 { b.push(c); }
}

fn commit_sel(n: usize) {
    let cands = CANDS.lock().unwrap();
    if n >= cands.len() { return; }
    let t = cands[n].text.clone();
    drop(cands);
    send_unicode(&t);
    println!("  [太阳] 已上屏: {}", t);
    BUF.lock().unwrap().clear();
    ui_refresh();
}

fn commit_raw() {
    let b = BUF.lock().unwrap().clone();
    if !b.is_empty() {
        send_unicode(&b);
        println!("  [太阳] 原文上屏: {}", b);
    }
    BUF.lock().unwrap().clear();
    ui_refresh();
}

fn move_sel(d: i32) {
    let len = CANDS.lock().unwrap().len();
    if len == 0 { return; }
    let cur = SEL.load(Ordering::Relaxed) as i32;
    let nx = (cur + d).clamp(0, len as i32 - 1);
    SEL.store(nx as usize, Ordering::Relaxed);
    PAGE.store((nx as usize) / PAGE_SIZE, Ordering::Relaxed);
    repaint();
}

fn page_flip(d: i32) {
    let len = CANDS.lock().unwrap().len();
    if len == 0 { return; }
    let psz = PAGE_SIZE as i32;
    let total = ((len as i32) + psz - 1) / psz;
    let cur = PAGE.load(Ordering::Relaxed) as i32;
    let np = (cur + d).clamp(0, total - 1);
    PAGE.store(np as usize, Ordering::Relaxed);
    let s0 = (np as usize) * PAGE_SIZE;
    SEL.store(if s0 < len { s0 } else { len - 1 }, Ordering::Relaxed);
    repaint();
}

// Shift单击(054): 英文态->回中文; 中文组词态->原文上屏+切英文直输
fn shift_tap() {
    if EN_MODE.load(Ordering::Relaxed) {
        set_en(false);
    } else {
        commit_raw();
        set_en(true);
    }
}

// 英文直输(053): F1切换, 按键全放行
fn set_en(on: bool) {
    EN_MODE.store(on, Ordering::Relaxed);
    BUF.lock().unwrap().clear();
    CANDS.lock().unwrap().clear();
    let wid = CAND_WND.load(Ordering::Relaxed);
    if wid != 0 { unsafe { let _ = ShowWindow(HWND(wid), SW_HIDE); } }
    println!("[太阳] {}", if on { "英文直输: 按键进应用, F1 切回中文" } else { "中文组词: 打拼音出候选" });
}

fn compose_off() {
    COMPOSING.store(false, Ordering::Relaxed);
    EN_MODE.store(false, Ordering::Relaxed);
    vocab_save(); // 055b: 组词结束即落盘, 压缩Ctrl+C尾部丢失面(SetConsoleCtrlHandler需新feature, 暂缓)
    BUF.lock().unwrap().clear();
    CANDS.lock().unwrap().clear();
    let wid = CAND_WND.load(Ordering::Relaxed);
    if wid != 0 { unsafe { let _ = ShowWindow(HWND(wid), SW_HIDE); } }
    println!("[太阳] 组词结束");
}

fn ui_refresh() {
    let buf = BUF.lock().unwrap().clone();
    let mut items: Vec<(String, i32, String)> = Vec::new();
    if !buf.is_empty() {
        if let Some(e) = ENGINE.get() {
            for acc in e.candidates(&buf, 24) {
                let word: String = acc.iter().map(|(wd, _)| wd.as_str()).collect();
                // 逐段查询(050b): 短语由词典词拼接, 整串不在词表 -> 各段各查再连
                let mut parts: Vec<String> = Vec::new();
                let ja_on = JA_MODE.load(Ordering::Relaxed);
                for (wd, _) in acc.iter() {
                    let g = gloss_of(e, ja_on, wd);
                    if !g.is_empty() && !parts.contains(&g) { parts.push(g); }
                    if parts.len() >= 4 { break; }
                }
                let gl = parts.join(if ja_on { " " } else { " · " });
                let gl = if gl.chars().count() > 44 {
                    format!("{}…", gl.chars().take(44).collect::<String>())
                } else { gl };
                let ww = est_w(&word, 17, 9);
                items.push((word, ww, gl));
            }
        }
    }
    let wid = CAND_WND.load(Ordering::Relaxed);
    if wid == 0 { return; }
    if buf.is_empty() && items.is_empty() {
        CANDS.lock().unwrap().clear();
        unsafe { let _ = ShowWindow(HWND(wid), SW_HIDE); }
        return;
    }
    let n = items.len().min(PAGE_SIZE) as i32;
    let ch = if n == 0 { 34 } else { 32 + n * 28 + 6 };
    // 曝光计数(055): 每次候选刷新, 前6词 seen+1(青简 record_exposure 语义)
    for (wd, _, _) in items.iter().take(6) { vocab_note(wd, false); }
    let maxrow = items.iter()
        .map(|(_, ww, gl)| 36 + ww + 12 + est_w(gl, 11, 6) + 14)
        .max().unwrap_or(320);
    *CANDS.lock().unwrap() = items.into_iter()
        .map(|(word, ww, g)| Cand { text: word, w: ww, gloss: g }).collect();
    PAGE.store(0, Ordering::Relaxed);
    SEL.store(0, Ordering::Relaxed);
    CUR_W.store(maxrow.clamp(WND_W, 560), Ordering::Relaxed);
    CUR_H.store(ch, Ordering::Relaxed);
    place_window();
    repaint();
}

fn place_window() {
    let wid = CAND_WND.load(Ordering::Relaxed);
    if wid == 0 { return; }
    let w = CUR_W.load(Ordering::Relaxed);
    let h = CUR_H.load(Ordering::Relaxed);
    unsafe {
        let scr_w = GetSystemMetrics(SM_CXSCREEN);
        let scr_h = GetSystemMetrics(SM_CYSCREEN);
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            flags: GUITHREADINFO_FLAGS(0),
            hwndActive: HWND::default(), hwndFocus: HWND::default(),
            hwndCapture: HWND::default(), hwndMenuOwner: HWND::default(),
            hwndMoveSize: HWND::default(), hwndCaret: HWND::default(),
            rcCaret: RECT::default(),
        };
        let _ = GetGUIThreadInfo(0, &mut gti);
        let (ax, ay_bot, ay_top, has_caret) = if gti.hwndCaret.0 != 0 {
            let mut p = POINT { x: gti.rcCaret.left, y: gti.rcCaret.bottom };
            let _ = ClientToScreen(gti.hwndCaret, &mut p);
            let mut t = POINT { x: 0, y: gti.rcCaret.top };
            let _ = ClientToScreen(gti.hwndCaret, &mut t);
            (p.x, p.y, t.y, true)
        } else {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            (p.x + 6, p.y + 8, p.y, false)
        };
        let mut x = ax;
        let mut y = ay_bot + 2;
        if x + w > scr_w - 8 { x = scr_w - 8 - w; }
        if x < 0 { x = 0; }
        if y + h > scr_h - 8 {
            y = if has_caret { ay_top - h - 6 } else { ay_bot - h - 10 };
            if y < 0 { y = 0; }
        }
        let hwnd = HWND(wid);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, 10, 10).out();
        let _ = SetWindowRgn(hwnd, rgn, true);
    }
}

fn repaint() {
    let wid = CAND_WND.load(Ordering::Relaxed);
    if wid != 0 {
        unsafe { let _ = InvalidateRect(HWND(wid), None, false); }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && COMPOSING.load(Ordering::Relaxed) {
        if GetForegroundWindow().0 != FGN.load(Ordering::Relaxed) {
            compose_off();
        } else {
            let st = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            if st.flags & LLKHF_INJECTED == KBDLLHOOKSTRUCT_FLAGS(0) {
                let down = wp.0 as u32 == 0x100 || wp.0 as u32 == 0x104;
                let vk = st.vkCode;
                // Shift单击检测(054): down->up无他键 = 单击切中英
                if vk == 0x10 || vk == 0xA0 || vk == 0xA1 {
                    if down {
                        SHIFT_DN.store(true, Ordering::Relaxed);
                        SHIFT_OT.store(false, Ordering::Relaxed);
                    } else if SHIFT_DN.load(Ordering::Relaxed) {
                        SHIFT_DN.store(false, Ordering::Relaxed);
                        if !SHIFT_OT.load(Ordering::Relaxed) { shift_tap(); }
                    }
                } else if down && SHIFT_DN.load(Ordering::Relaxed) {
                    SHIFT_OT.store(true, Ordering::Relaxed);
                }
                if down {
                    if EN_MODE.load(Ordering::Relaxed) {
                        if vk == 0x70 { set_en(false); return LRESULT(1); }
                    } else if handle_key(vk) { return LRESULT(1); }
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn fill(hdc: HDC, x: i32, y: i32, w: i32, h: i32, color: u32) {
    unsafe {
        let hb = CreateSolidBrush(COLORREF(color));
        let old = SelectObject(hdc, HGDIOBJ(hb.0));
        let _ = PatBlt(hdc, x, y, w, h, PATCOPY);
        SelectObject(hdc, old);
        let _ = DeleteObject(hb);
    }
}

fn text(hdc: HDC, x: i32, y: i32, s: &str, color: u32) {
    unsafe {
        let _ = SetTextColor(hdc, COLORREF(color));
        let u: Vec<u16> = s.encode_utf16().collect();
        let _ = TextOutW(hdc, x, y, &u);
    }
}

unsafe fn paint(hwnd: HWND) {
    let w = CUR_W.load(Ordering::Relaxed);
    let h = CUR_H.load(Ordering::Relaxed);
    let fbig = HFONT(*FONT_BIG.get_or_init(|| create_font(-17).0));
    let fsma = HFONT(*FONT_SMALL.get_or_init(|| create_font(-13).0));
    let hdc = GetDC(hwnd).out();
    fill(hdc, 0, 0, w, h, C_BG);
    SetBkMode(hdc, TRANSPARENT);
    let old = SelectObject(hdc, HGDIOBJ(fsma.0));
    fill(hdc, 0, 0, w, 1, C_BORDER);
    fill(hdc, 0, h - 1, w, 1, C_BORDER);
    fill(hdc, 0, 0, 1, h, C_BORDER);
    fill(hdc, w - 1, 0, 1, h, C_BORDER);
    let cands = CANDS.lock().unwrap().clone();
    let buf = BUF.lock().unwrap().clone();
    // 拼音行 + 光标(qingjian 样式)
    text(hdc, 14, 8, &buf, C_PY);
    fill(hdc, 14 + est_w(&buf, 13, 7) + 2, 9, 1, 14, C_CARET);
    // 竖排候选: 序号 | 词(大) | 英文释义(灰蓝)
    let total_pg = ((cands.len() as i32) + PAGE_SIZE as i32 - 1) / PAGE_SIZE as i32;
    let start = PAGE.load(Ordering::Relaxed) * PAGE_SIZE;
    let sel_abs = if cands.is_empty() { usize::MAX } else { SEL.load(Ordering::Relaxed).min(cands.len() - 1) };
    for (ri, i) in (start..cands.len()).take(PAGE_SIZE).enumerate() {
        let c = &cands[i];
        let ry = 32 + ri as i32 * 28;
        let sel = i == sel_abs;
        if sel { fill(hdc, 8, ry - 1, w - 16, 25, C_SELBG); }
        SelectObject(hdc, HGDIOBJ(fsma.0));
        text(hdc, 16, ry + 4, &format!("{}", ri + 1), C_NUM);
        SelectObject(hdc, HGDIOBJ(fbig.0));
        text(hdc, 36, ry, &c.text, if sel { C_SELTXT } else { C_TXT });
        if !c.gloss.is_empty() {
            SelectObject(hdc, HGDIOBJ(fsma.0));
            let mut gx = 36 + c.w + 12;
            // 徽标v2(055b): ☆独立橙色 + 截断后再补·Ln(055a截图教训: 先拼后截会吃掉徽标)
            let (sn, cm) = vocab_get(&c.text);
            let fresh = cm == 0 && sn < 3;
            if fresh {
                text(hdc, gx, ry + 4, "☆", C_NEW);
                gx += 14;
            }
            let lv = ENGINE.get().map(|e| freq_lv(e, &c.text)).unwrap_or(0);
            let badge = if lv > 0 { format!(" ·L{}", lv) } else { String::new() };
            let mut g = c.gloss.clone();
            while gx + est_w(&format!("{}{}", g, badge), 11, 6) > w - 14 && g.chars().count() > 2 { g.pop(); }
            g.push_str(&badge);
            text(hdc, gx, ry + 4, &g, if sel { C_SELGLOSS } else { C_GLOSS });
        }
    }
    if total_pg > 1 {
        let pg = format!("{}/{}", PAGE.load(Ordering::Relaxed) + 1, total_pg);
        SelectObject(hdc, HGDIOBJ(fsma.0));
        let pw = est_w(&pg, 11, 6);
        text(hdc, w - pw - 14, 9, &pg, C_NUM);
    }
    SelectObject(hdc, old);
    ReleaseDC(hwnd, hdc);
    let _ = ValidateRect(hwnd, None);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => { paint(hwnd); LRESULT(0) }
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() {
    unsafe { let _ = SetConsoleOutputCP(65001); }
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    println!("[太阳输入法 v0.15] 词典加载中...");
    let mut jm: HashMap<String, String> = HashMap::new();
    if let Ok(s) = fs::read_to_string(data.join("gloss-ja.tsv")) {
        for line in s.lines() {
            let mut it = line.splitn(3, '\t');
            if let (Some(w), _, Some(g)) = (it.next(), it.next(), it.next()) {
                jm.entry(w.trim().to_string())
                  .or_insert_with(|| g.trim().to_string());
            }
        }
    }
    println!("[ja] gloss-ja.tsv: {} 条", jm.len());
    let _ = JA_GLOSS.set(jm);
    let _ = VOCAB_PATH.set(data.join("vocab.tsv"));
    let mut vb: HashMap<String, (u32, u32)> = HashMap::new();
    if let Ok(s) = fs::read_to_string(data.join("vocab.tsv")) {
        for line in s.lines() {
            if line.starts_with('#') { continue; }
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() == 3 {
                if let (Ok(a), Ok(b)) = (c[1].parse::<u32>(), c[2].parse::<u32>()) {
                    vb.insert(c[0].trim().to_string(), (a, b));
                }
            }
        }
    }
    println!("[生词本] {} 条", vb.len());
    let _ = VOCAB.set(Mutex::new(vb));
    let _ = ENGINE.set(Engine::new(&data));
    println!("[OK] 就绪. Ctrl+Alt+M 开/关组词 (Ctrl+C 退出)");
    unsafe {
        let hinstance = HINSTANCE::default();
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(CS_DROPSHADOW.0),
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, PCWSTR(IDC_ARROW_VAL as *const u16)).out(),
            lpszClassName: w!("taiyang.cand"),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let wnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("taiyang.cand"), w!("太阳输入法"), WS_POPUP,
            0, 0, 360, 34, None, None, hinstance, None,
        ).out();
        CAND_WND.store(wnd.0, Ordering::Relaxed);
        if RegisterHotKey(None, 1, MOD_CONTROL | MOD_ALT, 0x4D).is_err() {
            eprintln!("[!!] Ctrl+Alt+M 注册失败(被占用?)");
            return;
        }
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinstance, 0).out();
        let _ = hook;
        println!("[hotkey] Ctrl+Alt+M组词 | F1英文直输 | F2释义语言 | 空格上屏 | Enter原文 | Esc退出");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_HOTKEY && msg.wParam.0 == 1 {
                let on = !COMPOSING.load(Ordering::Relaxed);
                COMPOSING.store(on, Ordering::Relaxed);
                if on {
                    BUF.lock().unwrap().clear();
                    FGN.store(GetForegroundWindow().0, Ordering::Relaxed);
                    println!("[太阳] 组词中: 打拼音 | 空格上屏 | Enter原文 | Esc退出 | 切窗自动关闭");
                    ui_refresh();
                } else { compose_off(); }
            }
        }
    }
}
