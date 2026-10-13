use serde::{Deserialize, Serialize};

/// DLL 发给 Server 的一次按键。M2 只带虚拟键码 (0x41..=0x5A 是 A..Z), ToUnicode 字符翻译 M3 补。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    /// Windows 虚拟键码 (VK_*)。
    pub virtual_key: u32,

    /// 按下时的修饰键状态。
    #[serde(default)]
    pub modifiers: KeyModifiers,
}

/// 修饰键状态 (协议自带的 bool 结构, 空值合法——与配置层的字符串版不同)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KeyModifiers {
    #[serde(default)]
    pub ctrl: bool,

    #[serde(default)]
    pub shift: bool,

    #[serde(default)]
    pub alt: bool,

    #[serde(default)]
    pub win: bool,
}

impl KeyModifiers {
    /// 有没有按着 Ctrl/Alt/Win——按着时不是打字, 一律放行。
    pub fn has_command_key(self) -> bool {
        self.ctrl || self.alt || self.win
    }
}

/// Server 对一次按键的处置: DLL 据此决定吃掉还是放行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyOutcome {
    /// 输入法消费了这次按键; DLL 吃掉, 应用收不到。
    Consumed,

    /// 输入法不处理; DLL 放行给应用。
    Passthrough,
}