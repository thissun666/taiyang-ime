# CLAUDE.md

本文件为 AI 编码助手（Claude Code 等）提供本仓库的工作指引。

## 仓库现状

Windows 拼音输入法，TSF DLL + Server 双进程架构。当前 0.1.0-alpha。基于青简 Qingjian 部分代码修改（GPL-3.0-or-later），修改与新增功能见 CHANGELOG.md。

## 目录地图

- taiyang-server：引擎 + IPC 服务进程。拼音解析、词库候选、日语变体表（0x87/0x97 合成键协议，main.rs 变体臂）、JA_MODE 释义切换
- taiyang-tsf：TSF DLL 前端。src/com/service/key_sink.rs（按键枢纽 ask_server、7 枚保留键、JIS 模式、五十音调色盘）、processor.rs（编辑会话、候选窗、保留键注册）
- 部署目标：%LOCALAPPDATA%\Taiyang\（taiyang_tsf.dll + server_boot.vbs 守护）
- 运行日志：%LOCALAPPDATA%\Taiyang\logs\tsf.log；开发日志：仓库外 ../PROJECT_LOG.txt

## 常用命令

    cargo build --release -p taiyang-tsf -p taiyang-server                                # 构建两个包
    Get-Process taiyang-server -EA SilentlyContinue | Stop-Process                        # 部署前先停 server
    Copy-Item target\release\taiyang_tsf.dll "$env:LOCALAPPDATA\Taiyang\" -Force           # 部署 DLL
    Start-Process wscript.exe -ArgumentList "`"$env:LOCALAPPDATA\Taiyang\server_boot.vbs`"" # 拉起 server
    Get-Content "$env:LOCALAPPDATA\Taiyang\logs\tsf.log" -Tail 60 -Encoding UTF8           # 取日志

## 约定（116 轮开发沉淀的硬纪律）

1. **签名铁证先行**：任何 windows-rs API 调用前，先查 %USERPROFILE%\.cargo\registry\src\...\windows-0.62.2\src\ 的函数签名（Option<HWND>、裸 i32、&mut [u16] 以源码为准）
2. **锚点纪律**：文本替换手术的锚命中数 ≠ 1 时必须红字阻断部署，严禁静默跳过（Ctrl+W 两轮静默跳过 → 从未上车 → 与记事本冲突案）
3. **版本标签**：激活日志带版本号（太阳 TSF vX.Y 已激活），测试第一步核对新窗激活行，杜绝在旧 DLL 窗户里测试
4. **探针先行**：新功能先插日志探针（进入/注册/应答/上屏四向），跑一次拿数据再修（变体三探针 → syn 直通道根因案）
5. **测试协议**：点进脚本新弹的测试专用记事本 → 先测后取原始日志（tail 不过滤）→ 按判决树分诊
6. **server 保活**：部署前停 server，部署后 VBS 拉起并确认进程在
7. **每轮记录**：PROJECT_LOG 一轮一条，含自首（错误根因）、铁证（源码/日志引用）、判决树
8. **颜色即 BGR**：COLORREF 是 0x00BBGGRR，按 RGB 直觉写会反色