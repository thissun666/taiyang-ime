use serde::{Deserialize, Serialize};

use super::key::KeyOutcome;
/// 一条候选 (M5 学习候选窗载荷): 词 + 释义 + 词性 + 等级。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// 候选词/词组。
    pub text: String,

    /// 英文释义 (gloss-en), 暂无则 None。
    #[serde(default)]
    pub gloss: Option<String>,

    /// 词性 (n./v./int. 等), 暂无则 None。
    #[serde(default)]
    pub pos: Option<String>,

    /// 词频等级 1..=5 (0=未知), 对应 typer freq_lv 分桶。
    #[serde(default)]
    pub level: u32,
}

/// Server 发给 DLL 的消息。M3: KeyResult 带 insert (要插进应用的字)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// 对 OpenSession 的答复。
    SessionOpened {
        session: u64,
    },

    /// 对 Key 的处理结果。
    KeyResult {
        session: u64,

        /// 吃掉还是放行。
        outcome: KeyOutcome,

        /// Some(text): 先把 text 插进应用, 再按 outcome 处置按键。
        /// (空格=首候选上屏, 回车=原文上屏, 缓冲非空遇其他键=让位上屏)
        #[serde(default)]
        insert: Option<String>,
        /// 本线程组句缓冲 (打字中的拼音); 空缓冲发 None。
        #[serde(default)]
        preedit: Option<String>,
        /// 候选列表 (M5); 本轮占位空表, 下轮填装。
        #[serde(default)]
        candidates: Vec<Candidate>,
    },

    /// 对 Commit 的答复。M3 仍保留形状。
    Committed {
        session: u64,
        text: Option<String>,
    },
}