//! 太阳输入法 Server。M3: myinput-core 真引擎 + 每连接组句缓冲。
//! 按键策略: 字母(a-z)进缓冲; 空格=查引擎上屏首候选; 回车=原文上屏;
//! 退格=删一个字母; Esc=清空; 缓冲非空遇其他键=原文先上屏再放行; 组合键一律放行。
//! 组句缓冲挂在连接上 (tid 跨进程会撞号), 断开即弃。

use std::collections::HashMap;
use std::fs;
use taiyang_platform::protocol::Candidate;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use myinput_core::Engine;
use taiyang_platform::protocol::{
    ClientMessage, KeyOutcome, KeyEvent, PROTOCOL_VERSION, ServerMessage, read_message,
    write_message,
};

const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;

fn main() {
    let data_dir = find_data_dir(std::env::args().nth(1).as_deref()).unwrap_or_else(|| {
        eprintln!("找不到 data/words.tsv。可显式指定: taiyang-server.exe <data目录>");
        process::exit(1);
    });
    let started = Instant::now();
    let engine = Arc::new(Engine::new(&data_dir));
    println!("太阳 Server M3 | 词库 {:?} | 加载 {:.2?}", data_dir, started.elapsed());

    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定 127.0.0.1 失败");
    let port = listener.local_addr().expect("取本地端口").port();
    let port_file = port_file_path();
    fs::create_dir_all(port_file.parent().unwrap()).expect("建 Taiyang 目录失败");
    fs::write(&port_file, port.to_string()).expect("写端口文件失败");
    println!("监听 127.0.0.1:{port} | 端口文件 {}", port_file.display());
    println!("字母=进缓冲 空格=上屏首候选 回车=上屏原文 Esc=清空 | Ctrl+C 退出");

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let engine = Arc::clone(&engine);
        thread::spawn(move || serve(stream, &engine));
    }
}

/// 词库目录: 命令行参数 > exe 祖先 > cwd 祖先, 找到含 words.tsv 的 data。
fn find_data_dir(arg: Option<&str>) -> Option<PathBuf> {
    if let Some(dir) = arg {
        let p = PathBuf::from(dir);
        if p.join("words.tsv").is_file() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    for anc in exe.ancestors().skip(1) {
        let p = anc.join("data");
        if p.join("words.tsv").is_file() {
            return Some(p);
        }
    }
    let cwd = std::env::current_dir().ok()?;
    for anc in cwd.ancestors() {
        let p = anc.join("data");
        if p.join("words.tsv").is_file() {
            return Some(p);
        }
    }
    None
}

fn port_file_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").expect("无 LOCALAPPDATA");
    PathBuf::from(base).join("Taiyang").join("server.port")
}

fn serve(mut stream: TcpStream, engine: &Engine) {
    // 组句缓冲 = 本连接私有状态; tid 跨进程会撞号, 所以不按 session 存。
    let mut buffer = String::new();
    loop {
        let Ok(Some(message)) = read_message::<_, ClientMessage>(&mut stream) else {
            return;
        };
        let reply = match message {
            ClientMessage::OpenSession { session, protocol } => {
                if protocol != PROTOCOL_VERSION {
                    println!("[{session}] 警告: DLL协议 {protocol} != {PROTOCOL_VERSION} (旧DLL只吃键不插字, 换新DLL后消失)");
                }
                buffer.clear();
                println!("[{session}] 会话开始");
                ServerMessage::SessionOpened { session }
            }
            ClientMessage::Key { session, event } => {
                let (outcome, insert) = handle_key(engine, session, &mut buffer, &event);
                if buffer.is_empty() { JA_MODE.store(false, Ordering::SeqCst); } // v16: 释义切换每轮有效
                let preedit = if buffer.is_empty() { None } else { Some(buffer.clone()) };
                let cands = current_cands(engine, &buffer); // v13: 统一候选源(引擎+单字母兜底)
                println!("[{session}] 查 {buffer:?} -> {} 条", cands.len()); // v13 统一后含兜底
                if !cands.is_empty() {
                    let top: Vec<&str> = cands.iter().take(3).map(|c| c.text.as_str()).collect();
                    println!("[{session}] 候选{}: {}", cands.len(), top.join(" "));
                }
                ServerMessage::KeyResult { session, outcome, insert, preedit, candidates: cands }
            }
            ClientMessage::Commit { session } => ServerMessage::Committed { session, text: None },
        };
        if write_message(&mut stream, &reply).is_err() {
            return;
        }
    }
}

/// M3 按键策略。返回 (吃/放, 要插进应用的字)。
fn handle_key(
    engine: &Engine,
    sid: u64,
    buffer: &mut String,
    event: &KeyEvent,
) -> (KeyOutcome, Option<String>) {
    if event.modifiers.has_command_key() {
        return (KeyOutcome::Passthrough, None); // Ctrl/Alt/Win 组合一律放行
    }
    match event.virtual_key {
        0x41..=0x5A => {
            let c = (event.virtual_key as u8) as char;
            buffer.push(c.to_ascii_lowercase());
            (KeyOutcome::Consumed, None)
        }
        VK_SPACE if !buffer.is_empty() => {
            let raw = std::mem::take(buffer);
            // v13: 统一候选源 — 引擎空时吃单字母兜底 (此前 best_candidate 绕过兜底 = 'n'+空格上屏n之根)
            let text = current_cands(engine, &raw)
                .into_iter()
                .next()
                .map(|c| c.text)
                .unwrap_or_else(|| raw.clone());
            println!("[{sid}] 上屏 {text} <- {raw}");
            (KeyOutcome::Consumed, Some(text))
        }
        VK_BACK if !buffer.is_empty() => {
            buffer.pop();
            (KeyOutcome::Consumed, None)
        }
        VK_RETURN if !buffer.is_empty() => {
            // v16: Shift轻点=原文直输 (中文拼音字母; 日语变体改走 Shift/Alt+1/2)
            let raw = std::mem::take(buffer);
            println!("[{sid}] 直输原文(Shift轻点) {raw}");
            (KeyOutcome::Consumed, Some(raw))
        }
        VK_ESCAPE if !buffer.is_empty() => {
            println!("[{sid}] 清空缓冲");
            buffer.clear();
            (KeyOutcome::Consumed, None)
        }
        0x71 if !buffer.is_empty() => {
            // v16: F2 = 释义切换 中文+英语 <-> 中文+日语词汇(假名); 候选原样不动; 每轮组句有效
            let on = !JA_MODE.load(Ordering::SeqCst);
            JA_MODE.store(on, Ordering::SeqCst);
            println!("[{sid}] 释义切换: {}", if on { "日语词汇(假名) (F2切回)" } else { "英语 (F2切日)" });
            (KeyOutcome::Consumed, None)
        }
        vk if !buffer.is_empty() && ((0x97..=0xA6).contains(&vk) || (0x87..=0x96).contains(&vk)) => {
            // v16: 日语变体提交 (DLL合成键: 0x97+=日语词 0x87+=其假名; 低3位=高亮候选序号)
            let (want_kana, idx) = if vk >= 0x97 { (false, (vk - 0x97) as usize) } else { (true, (vk - 0x87) as usize) };
            let cands = current_cands(engine, buffer.as_str());
            buffer.clear();
            match cands.get(idx).map(|c| c.text.clone()) {
                Some(cn) => {
                    let v = if want_kana { ja_kana_of(&cn) } else { ja_word_of(&cn) };
                    let t = v.unwrap_or_else(|| cn.clone()); // 无映射退中文, 不丢键
                    println!("[{sid}] 日语变体上屏 {t} <- {cn} (序{}{})", idx + 1, if want_kana { "假名" } else { "词" });
                    (KeyOutcome::Consumed, Some(t))
                }
                None => (KeyOutcome::Consumed, None),
            }
        }        vk @ 0x31..=0x39 if !buffer.is_empty() => {
            let idx = (vk - 0x31) as usize;
            let cands = current_cands(engine, buffer.as_str()); // v13: 统一候选源(088 kk兜底升级)
            if idx < cands.len() {
                let text = cands[idx].text.clone();
                buffer.clear();
                println!("[{sid}] 选词上屏 {text} (数字{})", idx + 1);
                (KeyOutcome::Consumed, Some(text))
            } else {
                let raw = std::mem::take(buffer);
                println!("[{sid}] 让位上屏 {raw} (vk={vk:#x})");
                (KeyOutcome::Passthrough, Some(raw))
            }
        }
        vk if !buffer.is_empty() => {
            // v11: 让位不再把拼音提交成文本 (旧DLL宿主 Ctrl/PrtSc 落拼音之根) — 丢弃缓冲放行
            let raw = std::mem::take(buffer);
            println!("[{sid}] 让位丢弃 {raw} (vk={vk:#x})");
            (KeyOutcome::Passthrough, None)
        }
        _ => (KeyOutcome::Passthrough, None),
    }
}

/// 首候选拼成串; 引擎没辙就原样吐回 (宁可出字母, 不丢输入)。
#[allow(dead_code)]
fn best_candidate(engine: &Engine, raw: &str) -> String {
    match engine.candidates(raw, 1).into_iter().next() {
        Some(acc) if !acc.is_empty() => acc.into_iter().map(|(w, _)| w).collect(),
        _ => raw.to_string(),
    }
}

// ===== M5: 候选富化 v2 (typer 清洗语义对齐 + 日语 F2) =====use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static JA_MODE: AtomicBool = AtomicBool::new(false);
static JA_GLOSS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 懒加载 gloss-ja.tsv (exe 祖先找 data/, 与词库发现同源)。
fn ja_gloss() -> &'static HashMap<String, String> {
    JA_GLOSS.get_or_init(|| {
        let mut map = HashMap::new();
        let mut dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
        let mut found: Option<PathBuf> = None;
        while let Some(d) = dir {
            if d.join("data").join("gloss-ja.tsv").exists() { found = Some(d.join("data")); break; }
            dir = d.parent().map(|p| p.to_path_buf());
        }
        if let Some(d) = found {
            if let Ok(s) = std::fs::read_to_string(d.join("gloss-ja.tsv")) {
                for line in s.lines() {
                    let mut it = line.splitn(3, '\t');
                    if let (Some(w), _, Some(g)) = (it.next(), it.next(), it.next()) {
                        map.entry(w.trim().to_string()).or_insert_with(|| g.trim().to_string());
                    }
                }
            }
        }
        println!("[ja] gloss-ja.tsv: {} 条", map.len());
        map
    })
}

fn fmt_ja(s: &str) -> String {
    match s.split_once('|') {
        Some((w, k)) => format!("{}（{}）", w.trim(), k.trim()),
        None => s.trim().to_string(),
    }
}

/// en: 单词前2义 " · " 连 (053); 空则逐字兜底每字第一义 (054)。
fn gloss_of_en(engine: &Engine, wd: &str) -> String {
    if let Some(s) = engine.gloss(wd) {
        let v: Vec<&str> = s.split(';').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
        let g = v.iter().take(2).cloned().collect::<Vec<_>>().join(" · ");
        if !g.is_empty() { return g; }
    }
    if wd.chars().count() > 1 {
        let mut pcs: Vec<String> = Vec::new();
        for c in wd.chars() {
            if let Some(s) = engine.gloss(c.to_string().as_str()) {
                let g0 = s.split(';').next().unwrap_or("").trim().to_string();
                if !g0.is_empty() && !pcs.contains(&g0) { pcs.push(g0); }
            }
        }
        return pcs.join(" · ");
    }
    String::new()
}

/// ja: fmt_ja 汉字（假名）; 空则逐字兜底, 空格连 (typer 同款)。
fn gloss_of_ja(wd: &str) -> String {
    let map = ja_gloss();
    if let Some(s) = map.get(wd) {
        let g = fmt_ja(s);
        if !g.is_empty() { return g; }
    }
    if wd.chars().count() > 1 {
        let mut pcs: Vec<String> = Vec::new();
        for c in wd.chars() {
            if let Some(s) = map.get(c.to_string().as_str()) {
                let g0 = fmt_ja(s);
                if !g0.is_empty() && !pcs.contains(&g0) { pcs.push(g0); }
            }
        }
        return pcs.join(" ");
    }
    String::new()
}

/// 词频等级分桶 (055a: 3000/1500/600/200, 与 typer 逐字核对一致)。
fn freq_lv(engine: &Engine, w: &str) -> u32 {
    match engine.freq(w) {
        Some(f) if f >= 3000.0 => 1,
        Some(f) if f >= 1500.0 => 2,
        Some(f) if f >= 600.0 => 3,
        Some(f) if f >= 200.0 => 4,
        Some(_) => 5,
        None => 0,
    }
}

/// raw -> 候选: 单词=前2义全义; 多词=每词第一义 · 连 <=4段 (050); 32字截断。
fn build_candidates(engine: &Engine, raw: &str) -> Vec<Candidate> {
    let ja = JA_MODE.load(Ordering::Relaxed);
    engine
        .candidates(raw, 16)
        .into_iter()
        .filter_map(|acc| {
            if acc.is_empty() {
                return None;
            }
            let text: String = acc.iter().map(|(w, _)| w.as_str()).collect();
            let mut gloss = match acc.len() {
                1 => {
                    if ja { gloss_of_ja(&acc[0].0) } else { gloss_of_en(engine, &acc[0].0) }
                }
                _ => {
                    let parts: Vec<String> = acc
                        .iter()
                        .filter_map(|(w, _)| {
                            let g = if ja {
                                gloss_of_ja(w)
                            } else {
                                let f = engine
                                    .gloss(w)
                                    .map(|s| s.split(';').next().unwrap_or("").trim().to_string())
                                    .unwrap_or_default();
                                if f.is_empty() { gloss_of_en(engine, w) } else { f }
                            };
                            if g.is_empty() { None } else { Some(g) }
                        })
                        .take(4)
                        .collect();
                    parts.join("·")
                }
            };
            if gloss.chars().count() > 32 {
                gloss = gloss.chars().take(32).collect();
            }
            let gloss = if gloss.is_empty() { None } else { Some(gloss) };
            let level = freq_lv(engine, &text);
            Some(Candidate { text, gloss, pos: None, level })
        })
        .collect()
}

// ===== v12: 单字母/无匹配兜底常用字表 (R4实锤: 引擎切分对纯单字母返回空/ga经pfx出8条; n/k/s/kk 悬案总根) =====
static FALLBACK_CHARS: &[(&str, &str)] = &[
    ("啊","a"),("爱","ai"),("安","an"),("傲","ao"),
    ("不","bu"),("吧","ba"),("白","bai"),("办","ban"),("帮","bang"),("报","bao"),("杯","bei"),("本","ben"),("边","bian"),("别","bie"),
    ("吃","chi"),("出","chu"),("错","cuo"),("菜","cai"),("草","cao"),("茶","cha"),("车","che"),
    ("的","de"),("大","da"),("到","dao"),("得","de"),("地","di"),("点","dian"),("东","dong"),("读","du"),("多","duo"),
    ("饿","e"),("恩","en"),("二","er"),
    ("发","fa"),("饭","fan"),("飞","fei"),("分","fen"),("风","feng"),
    ("该","gai"),("哥","ge"),("给","gei"),("跟","gen"),("狗","gou"),("姑","gu"),("关","guan"),("贵","gui"),("国","guo"),
    ("好","hao"),("喝","he"),("黑","hei"),("很","hen"),("红","hong"),("后","hou"),("花","hua"),("会","hui"),("火","huo"),("海","hai"),
    ("就","jiu"),("几","ji"),("家","jia"),("叫","jiao"),("街","jie"),("进","jin"),("军","jun"),
    ("可","ke"),("开","kai"),("看","kan"),("刻","ke"),("客","ke"),("课","ke"),("口","kou"),("哭","ku"),("快","kuai"),("宽","kuan"),("空","kong"),("困","kun"),("阔","kuo"),
    ("了","le"),("乐","le"),("老","lao"),("里","li"),("两","liang"),("六","liu"),("龙","long"),("路","lu"),("绿","lv"),
    ("没","mei"),("马","ma"),("买","mai"),("猫","mao"),("门","men"),("米","mi"),("面","mian"),("明","ming"),("木","mu"),
    ("你","ni"),("那","na"),("拿","na"),("奶","nai"),("南","nan"),("脑","nao"),("内","nei"),("能","neng"),("鸟","niao"),("牛","niu"),("农","nong"),
    ("哦","o"),("偶","ou"),
    ("怕","pa"),("跑","pao"),("朋","peng"),("票","piao"),("品","pin"),("平","ping"),("破","po"),
    ("去","qu"),("七","qi"),("骑","qi"),("前","qian"),("桥","qiao"),("请","qing"),("全","quan"),
    ("人","ren"),("然","ran"),("日","ri"),("肉","rou"),("如","ru"),
    ("是","shi"),("三","san"),("山","shan"),("身","shen"),("生","sheng"),("十","shi"),("手","shou"),("书","shu"),("水","shui"),("说","shuo"),("四","si"),("算","suan"),("所","suo"),
    ("他","ta"),("她","ta"),("台","tai"),("天","tian"),("条","tiao"),("听","ting"),("同","tong"),("头","tou"),("图","tu"),
    ("我","wo"),("外","wai"),("王","wang"),("喂","wei"),("文","wen"),("五","wu"),
    ("想","xiang"),("下","xia"),("西","xi"),("先","xian"),("小","xiao"),("写","xie"),("心","xin"),("星","xing"),("学","xue"),
    ("有","you"),("要","yao"),("爷","ye"),("一","yi"),("音","yin"),("用","yong"),("鱼","yu"),("月","yue"),("云","yun"),("烟","yan"),("羊","yang"),
    ("在","zai"),("早","zao"),("怎","zen"),("这","zhe"),("真","zhen"),("直","zhi"),("中","zhong"),("周","zhou"),("猪","zhu"),("子","zi"),("走","zou"),("足","zu"),("嘴","zui"),("昨","zuo"),
];
/// v13: 统一候选源 = 引擎查询 + 单字母/无匹配兜底 (空格/数字/Key应答 单一真相)。
fn current_cands(engine: &Engine, buffer: &str) -> Vec<Candidate> {

    let cands = build_candidates(engine, buffer);
    if cands.is_empty() && !buffer.is_empty() {
        fallback_candidates(engine, buffer.chars().next().unwrap())
    } else {
        cands
    }
}

/// 按首字母过滤常用字顶替空候选 (v13: 真释义 — gloss_of_en/ja 现查, 缺则 常用字 拼音)。
fn fallback_candidates(engine: &Engine, initial: char) -> Vec<Candidate> {
    FALLBACK_CHARS
        .iter()
        .filter(|x| x.1.starts_with(initial))
        .take(8)
        .map(|&(w, py)| {
            let g = if JA_MODE.load(Ordering::Relaxed) {
                gloss_of_ja(w)
            } else {
                gloss_of_en(engine, w)
            };
            let g = g.trim().to_string();
            Candidate {
                text: w.to_string(),
                gloss: Some(if g.is_empty() { format!("常用字 {py}") } else { g }),
                pos: None,
                level: 0,
            }
        })
        .collect()
}
// ===== v16: 日语变体提交 (Shift/Alt+1=日语词 2=假名; 直读 gloss-ja, 不走罗马字) =====
fn ja_parse(g: &str) -> (String, String) {
    if let Some((k, r)) = g.split_once('|') {
        (k.trim().to_string(), r.trim().to_string())
    } else if let Some(i) = g.find('（') {
        let rest = &g[i + '（'.len_utf8()..];
        let inner = rest.strip_suffix('）').unwrap_or(rest);
        (g[..i].trim().to_string(), inner.trim().to_string())
    } else {
        (g.trim().to_string(), String::new())
    }
}
fn ja_word_of(w: &str) -> Option<String> {
    ja_gloss().get(w).map(|g| { let (k, r) = ja_parse(g); if k.is_empty() { r } else { k } }).filter(|s| !s.is_empty())
}
fn ja_kana_of(w: &str) -> Option<String> {
    ja_gloss().get(w).map(|g| { let (k, r) = ja_parse(g); if r.is_empty() { k } else { r } }).filter(|s| !s.is_empty())
}