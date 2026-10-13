# 更新日志

一个版本一节，标题「版本 · 日期 · 渠道」（stable / beta / alpha / rc），正文一行一条、面向用户的措辞。

## 0.1.0 · 2026-10-13 · alpha

- 首个内测版。Windows TSF 拼音输入法：整句输入、简拼、候选翻页与高亮、组句编辑
- 日语学习双变体：候选中 Ctrl+1 上屏日语词汇、Ctrl+2 上屏假名；Ctrl+Shift+1/2 第二组键位；F2 切换释义语言
- 五十音调色盘：平/片假名两页（清音浊音半浊音拗音促音小写长音），页头切换，Ctrl+Shift+3 或反引号开关，Ctrl+Shift+5 关闭
- 调色盘体验：DPI 自适应（AdjustWindowRectExForDpi 精确反推窗口尺寸）、Yu Gothic UI 大字号、矩形居中绘制、按下橙色高亮与浅橙余痕、双缓冲无闪烁
- JIS 假名键盘模式：Ctrl+Shift+4 开启后按 JIS 布局直出假名，Esc 退出
- Shift+字母 / CapsLock 英文直输；全角中文标点；PrtSc 截图净场
- server 与 DLL 双进程 IPC 架构：VBS 守护进程拉起，僵尸连接自动重连
- 已知问题：候选窗口在部分系统应用（任务栏搜索等）可能被遮挡；变体提交需在候选挂着时按下（组句上屏后无效）
- 本版基于青简 Qingjian（GPL-3.0）部分代码修改，修改声明见 README