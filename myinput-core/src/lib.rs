//! myinput-core v0.1 - Python 原型 v1.1 引擎移植 (Rust 2B 第一阶段)
//! 数据: data/words.tsv (26万词) + data/gloss-en.tsv (v3)
//! 验收: 测试对照 Python 八项自检

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub type Acc = Vec<(String, f64)>;

pub struct Engine {
    idx: HashMap<String, Vec<(String, f64)>>,
    freq: HashMap<String, f64>,
    syllables: HashSet<String>,
    gloss: HashMap<String, String>,
    pfx: HashMap<String, Vec<String>>,
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn load_words(
    path: &Path,
) -> (
    HashMap<String, Vec<(String, f64)>>,
    HashMap<String, f64>,
    HashSet<String>,
) {
    let text = fs::read_to_string(path).expect("words.tsv 缺失");
    let (mut idx, mut freq, mut syls) =
        (HashMap::new(), HashMap::new(), HashSet::new());
    let (mut py_col, mut probed) = (1usize, false);
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        if !probed {
            py_col = if cols[1].trim().bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                2
            } else {
                1
            };
            probed = true;
        }
        let f_col = if py_col == 1 { 2 } else { 1 };
        let word = cols[0].trim().to_string();
        let py = cols[py_col].trim().to_lowercase();
        let f = cols[f_col].trim().parse::<f64>().unwrap_or(1.0);
        if word.is_empty() || py.is_empty() {
            continue;
        }
        let toks: Vec<&str> = py.split_whitespace().collect();
        for tk in &toks {
            syls.insert(tk.to_string());
        }
        // v17(M6开胃): 单字多音字逐音节索引 (都 du dou 经 dou 可达); 多字词仍整串键
        let keys: Vec<String> = if word.chars().count() == 1 {
            toks.iter().map(|s| s.to_string()).collect()
        } else {
            vec![toks.join(" ")]
        };
        for key in &keys {
            let e = idx.entry(key.clone()).or_insert_with(Vec::new);
            if !e.iter().any(|(w, _)| *w == word) {
                e.push((word.clone(), f));
            }
        }
        freq.entry(word).or_insert(f);
    }
    for v in idx.values_mut() {
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(40);
    }
    (idx, freq, syls)
}

fn load_gloss(path: &Path) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(text) = fs::read_to_string(path) {
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut it = line.splitn(2, '\t');
            if let (Some(w), Some(g)) = (it.next(), it.next()) {
                m.insert(w.trim().to_string(), g.trim().to_string());
            }
        }
    }
    m
}

impl Engine {
    pub fn new(data_dir: &Path) -> Engine {
        let (idx, freq, syllables) = load_words(&data_dir.join("words.tsv"));
        let gloss = load_gloss(&data_dir.join("gloss-en.tsv"));
        let sylsv: Vec<String> = syllables.iter().cloned().collect();
        let mut pfx: HashMap<String, Vec<String>> = HashMap::new();
        for a in &sylsv {
            for b in &sylsv {
                let (la, lb) = (a.chars().count(), b.chars().count());
                if lb > la && lb - la <= 2 && b.starts_with(a.as_str()) {
                    pfx.entry(a.clone()).or_default().push(b.clone());
                }
            }
        }
        Engine { idx, freq, syllables, gloss, pfx }
    }

    pub fn freq(&self, w: &str) -> Option<f64> {
        self.freq.get(w).copied()
    }

    pub fn gloss(&self, w: &str) -> Option<&str> {
        self.gloss.get(w).map(|s| s.as_str())
    }

    fn greedy_seg(&self, s: &str) -> Option<Vec<String>> {
        let ch: Vec<char> = s.chars().collect();
        let n = ch.len();
        let mut f = vec![false; n + 1];
        let mut pick = vec![0usize; n + 1];
        f[n] = true;
        for i in (0..n).rev() {
            let maxl = 6.min(n - i);
            for l in (1..=maxl).rev() {
                let sub: String = ch[i..i + l].iter().collect();
                if f[i + l] && self.syllables.contains(&sub) {
                    f[i] = true;
                    pick[i] = l;
                    break;
                }
            }
        }
        if !f[0] {
            return None;
        }
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < n {
            let l = pick[i];
            out.push(ch[i..i + l].iter().collect::<String>());
            i += l;
        }
        Some(out)
    }

    fn rec(
        &self,
        syls: &[String],
        pos: usize,
        acc: Acc,
        ssum: f64,
        smin: Option<f64>,
        out: &mut Vec<(f64, f64, Acc)>,
    ) {
        if out.len() >= 600 {
            return;
        }
        if pos == syls.len() {
            let avg = ssum / acc.len() as f64;
            out.push((smin.unwrap_or(0.0), avg, acc));
            return;
        }
        let maxn = 4.min(syls.len() - pos);
        for n in (1..=maxn).rev() {
            let key = syls[pos..pos + n].join(" ");
            if let Some(entries) = self.idx.get(&key) {
                for (w, s) in entries.iter().take(12) {
                    // 单字轻罚(053): 词典词压过单字组合
                    let s1 = if w.chars().count() == 1 { *s * 0.6 } else { *s };
                    let mut a2 = acc.clone();
                    a2.push((w.clone(), s1));
                    let m2 = match smin {
                        None => s1,
                        Some(m) => m.min(s1),
                    };
                    self.rec(syls, pos + n, a2, ssum + s1, Some(m2), out);
                }
            }
        }
    }

    pub fn query(&self, syls: &[&str], k: usize) -> Vec<Acc> {
        let syls: Vec<String> =
            syls.iter().map(|s| s.trim().to_lowercase()).collect();
        let mut out: Vec<(f64, f64, Acc)> = Vec::new();
        self.rec(&syls, 0, Vec::new(), 0.0, None, &mut out);
        // 末音节前缀扩展(053b): 音节级整段替换重查(词典词级), 末词0.5罚分
        if syls.len() >= 2 {
            let last = syls.last().unwrap().clone();
            if let Some(exts) = self.pfx.get(&last) {
                for ext in exts {
                    let mut ms = syls.clone();
                    let ln = ms.len();
                    ms[ln - 1] = ext.clone();
                    let mut o2: Vec<(f64, f64, Acc)> = Vec::new();
                    self.rec(&ms, 0, Vec::new(), 0.0, None, &mut o2);
                    for (_, _, mut acc) in o2 {
                        if let Some(lw) = acc.last_mut() { lw.1 *= 0.5; }
                        let sc: Vec<f64> = acc.iter().map(|(_, s)| *s).collect();
                        let mn = sc.iter().cloned().fold(f64::INFINITY, f64::min);
                        let av = sc.iter().sum::<f64>() / sc.len().max(1) as f64;
                        out.push((mn, av, acc));
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            a.2.len()
                .cmp(&b.2.len())
                .then(b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
                .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut seen: HashSet<String> = HashSet::new();
        let mut res: Vec<Acc> = Vec::new();
        for (_, _, acc) in out {
            let names: Vec<&str> = acc.iter().map(|(w, _)| w.as_str()).collect();
            let key = names.join("\u{1}");
            let jkey = format!("J{}", names.concat());
            if seen.contains(&key) || seen.contains(&jkey) {
                continue;
            }
            seen.insert(key);
            seen.insert(jkey);
            res.push(acc);
            if res.len() == k {
                break;
            }
        }
        res
    }

    fn cjk_cands(&self, text: &str, k: usize) -> Vec<Acc> {
        let mut res: Vec<Acc> = Vec::new();
        if let Some(&f) = self.freq.get(text) {
            res.push(vec![(text.to_string(), f)]);
        }
        let mut pref: Vec<(&String, &f64)> = self
            .freq
            .iter()
            .filter(|(w, _)| w.len() > text.len() && w.starts_with(text))
            .collect();
        pref.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (w, f) in pref.into_iter().take(k.saturating_sub(res.len())) {
            res.push(vec![(w.clone(), *f)]);
        }
        res
    }

    pub fn candidates(&self, text: &str, k: usize) -> Vec<Acc> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if text.chars().any(is_cjk) {
            return self.cjk_cands(text, k);
        }
        let low = text.to_lowercase();
        if low.contains(' ') || low.contains('\'') {
            let syls: Vec<&str> = low
                .split(|c| c == ' ' || c == '\'')
                .filter(|s| !s.is_empty())
                .collect();
            return self.query(&syls, k);
        }
        if low.chars().count() > 1 {
            if let Some(seg) = self.greedy_seg(&low) {
                if seg.len() > 1 {
                    let refs: Vec<&str> = seg.iter().map(|s| s.as_str()).collect();
                    return self.query(&refs, k);
                }
            }
        }
        self.query(&[low.as_str()], k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn eng() -> &'static Engine {
        static E: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();
        E.get_or_init(|| Engine::new(&data_dir()))
    }

    fn joined(a: &Acc) -> String {
        a.iter().map(|(w, _)| w.as_str()).collect()
    }

    #[test]
    fn t1_cjk_exact_first() {
        let e = eng();
        let c = e.candidates("今天", 9);
        assert!(!c.is_empty(), "今天 无候选");
        assert_eq!(joined(&c[0]), "今天");
    }

    #[test]
    fn t2_cjk_prefix_order() {
        let e = eng();
        let c = e.candidates("今天", 9);
        assert!(c.iter().skip(1).all(|a| joined(a).starts_with("今天")));
    }

    #[test]
    fn t3_greedy_seg() {
        let e = eng();
        let c = e.candidates("jintian", 9);
        assert!(!c.is_empty(), "jintian 无候选");
        assert_eq!(joined(&c[0]), "今天");
    }

    #[test]
    fn t4_pinyin_sorted() {
        let e = eng();
        let c = e.candidates("shi", 9);
        assert!(c.len() >= 2, "shi 候选不足");
        assert!(c[0][0].1 >= c[1][0].1, "词频序错乱");
    }

    #[test]
    fn t5_gloss() {
        let e = eng();
        let g = e.gloss("你好").expect("gloss 未命中 你好");
        assert!(g.contains("hello"), "你好 gloss 异常: {}", g);
    }

    #[test]
    fn t6_whole_word_first() {
        let e = eng();
        let c = e.candidates("qing ji", 9);
        assert!(
            c.iter().take(5).any(|a| a.len() == 1),
            "整词候选未进前5"
        );
    }

    #[test]
    fn t7_long_sentence() {
        let e = eng();
        let c = e.candidates("jin tian chi shen me", 9);
        assert!(!c.is_empty(), "长句无候选");
        assert_eq!(c[0].len(), 3, "长句切分异常: {:?}", c[0]);
        assert_eq!(c[0][0].0, "今天");
    }

    #[test]
    fn t8_combo_dedupe() {
        let e = eng();
        let c = e.candidates("da jia", 9);
        let mut seen = HashSet::new();
        assert!(c.iter().all(|a| seen.insert(joined(a))), "组合重复");
    }

    #[test]
    fn t9_last_syl_prefix() {
        let e = eng();
        let c = e.candidates("qing ji", 12);
        let js: Vec<String> = c.iter().map(joined).collect();
        assert_eq!(js[0], "情急", "精确匹配应第一: {:?}", js);
        assert!(js.iter().take(12).any(|w| w == "情节"), "前缀扩展未生效: {:?}", js);
        assert!(js.iter().take(12).any(|w| w == "清洁"), "前缀扩展未生效: {:?}", js);
    }
}
