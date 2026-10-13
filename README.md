<p align="center">
  <h1 align="center">太阳输入法 Taiyang IME</h1>
  <p align="center"><strong>中文拼音输入，顺手学日语。</strong></p>
  <p align="center">
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later"></a>
    <img src="https://img.shields.io/badge/Windows-10%2F11-blue" alt="Windows 10/11">
    <img src="https://img.shields.io/badge/Rust-stable-orange" alt="Rust">
  </p>
</p>

太阳是一款 Windows 拼音输入法（TSF）：打拼音、选候选，候选可以一键上屏为**日语词汇或假名**——语言学习发生在日常输入里。内置五十音图调色盘与 JIS 假名键盘，日语字符即点即输。

> 本项目基于 [青简 Qingjian](https://github.com/qingjian-team/qingjian)（GPL-3.0）的部分代码修改发展而来，在此致谢。依据 GPL-3.0-or-later 同样开源，修改内容见 [CHANGELOG.md](CHANGELOG.md)。

## 功能

- **拼音整句输入**：一口气打完一句再上屏，简拼、候选翻页、高亮选择、组句编辑
- **日语变体上屏**：候选挂着时 Ctrl+1 上屏日语词、Ctrl+2 上屏假名；F2 切换释义语言（英语/日语）
- **五十音调色盘**：平假名/片假名两页，清音浊音半浊音拗音促音全覆盖；DPI 自适应、按下高亮动画、双缓冲无闪烁
- **JIS 假名键盘模式**：开启后按 JIS 布局直出假名，Esc 退出；浊音拗音走调色盘
- **英文直输**：Shift+字母 / CapsLock，不打断中文组句；全角中文标点

## 快捷键

| 快捷键 | 功能 |
|---|---|
| Ctrl+1 / Ctrl+2 | 候选中：上屏日语词 / 假名 |
| Ctrl+Shift+1 / 2 | 同上（第二组键位） |
| Ctrl+Shift+3 或反引号 | 开关五十音调色盘 |
| Ctrl+Shift+4 | 开关 JIS 假名键盘模式 |
| Ctrl+Shift+5 | 关闭调色盘 |
| Esc | 退出假名模式 / 清空组句 |
| F2 | 切换释义语言 |
| 方向键 | 左右翻页 / 上下移动高亮 |
| 空格 | 提交高亮候选 |

## 架构与构建

- taiyang-server：引擎 + IPC 服务进程（VBS 守护拉起，拼音解析、词库候选、日语变体表）
- taiyang-tsf：Windows TSF 输入法 DLL（按键处理、候选窗、调色盘、保留键）

    cargo build --release -p taiyang-tsf -p taiyang-server

构建产物 taiyang_tsf.dll 部署到 %LOCALAPPDATA%\Taiyang\ 并注册 TSF 后，在系统输入法列表中选择。开发与部署细节见 [CLAUDE.md](CLAUDE.md)。

## 数据与隐私

拼音转换与候选全部在本机完成，不上传任何按键内容；运行日志只写本机 %LOCALAPPDATA%\Taiyang\logs\。

## 许可证

代码以 [GPL-3.0-or-later](LICENSE) 发布；项目名称与图标不包含在代码授权中。基于青简 Qingjian 的修改已按 GPL 第 5 条声明。