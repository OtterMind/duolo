# 架构

## Crate 划分

| Crate | 职责 | 依赖 |
|---|---|---|
| `duolo-core` | 领域模型（项目、任务、Agent）、配置、`state.json` 持久化、文本工具 | 无 UI、无进程 |
| `duolo-git` | git 命令行封装：仓库定位、worktree、改动统计 | git |
| `duolo-pty` | 在 PTY 里运行 Agent CLI，VT 解析维护屏幕，按输出节奏和屏幕内容识别状态 | portable-pty、vt100 |
| `duolo-voice` | 语音：调用外部录音转写命令、调用整理命令 | core |
| `duolo-engine` | 调度：任务生命周期、worktree、Agent 启动与恢复、会话管理 | core、git、pty |
| `duolo-app` | 桌面应用后端（Tauri）：把 Engine 的能力作为命令提供给界面，推送状态快照和 Agent 输出 | engine、voice、tauri |
| `app/` | 桌面应用界面（React + xterm.js）：侧边栏、首页、任务页、语音 | 只通过 Tauri 命令访问后端 |
| `duolo-tui` | 终端界面（`duolo tui`，适合 SSH）：自己做 VT 合成 | engine、voice、ratatui |
| `duolo-cli` | `duolo` 命令行入口，默认打开桌面应用 | 以上全部 |

依赖只能自上而下：`core` 不依赖任何人，`engine` 不依赖 UI。

## 数据流

桌面应用：

```
xterm.js 按键 ─▶ invoke("write") ─▶ Engine::write ─▶ PTY ─▶ claude / codex
                                                        │
xterm.js 画面 ◀─ Channel（原始字节）◀─ Session::attach ◀─┤ 读线程
侧边栏 ◀─ "snapshot" 事件 ◀─ 每 250ms 生成快照，变化才推送 ◀─ vt100 解析器（状态识别）
```

- Agent 的原始输出原样交给 xterm.js 渲染；`attach` 先重放已有输出再实时推送，两者在同一把锁内完成，不丢不重。
- 每个 Agent 在界面里有一个常驻的 xterm 实例，切换任务只是挂到另一个位置，历史和光标都保留。
- 每次启动进程都有新的会话编号（`AgentView.session`），界面看到编号变化就重新订阅。
- 关闭窗口只隐藏，Agent 继续运行；`⌘Q` 才结束所有进程。
- 从 Dock 启动的应用拿不到 shell 的 PATH，启动时从登录 shell 读取（`shell_env.rs`）。

终端界面：

```
键盘 ─▶ input::handle ─▶ App 操作 ─▶ Engine ─▶ Session.write ─▶ PTY ─▶ claude / codex
                                                                     │
画面 ◀─ ui::render ◀─ Session.with_screen ◀─ vt100::Parser ◀─ 读线程 ◀┘
                                                   └─ OutputHook ─▶ 标记需要重绘
```

- 每个 Agent 一个 `Session`：后台读线程把输出喂给 VT 解析器，并记录最近输出时间。
- 界面按需重绘：有新输出、有按键，或者每 250ms 刷新一次转圈图标和用时。
- 语音的转写和整理在后台线程执行，结果通过 channel 回到界面线程。

## 状态识别

`duolo-pty::detect::classify` 按以下顺序判断：

1. 进程已退出 → 已结束；
2. 屏幕底部出现确认选项（`1. Yes`、`Do you want to proceed` 等）→ 等你确认；
3. 出现 `esc to interrupt`，或最近 1.5 秒有输出 → 运行中；
4. 否则 → 等待输入。

## 扩展点

- **守护进程 `duolod`**：把 `Engine` 搬进常驻进程，桌面应用和 TUI 通过 Unix socket 调用同样的方法；`duolo-pty` 无需改动。
  拆分后退出应用不会结束 Agent，手机端也能连接。`duolo-app/src/view.rs` 的快照格式就是以后的协议基础。
- **网页端 / 手机端**：`app/` 的界面只依赖 `lib/api.ts` 这一层，换成 WebSocket 实现即可复用。
- **hooks 和会话日志**：在 `duolo-pty::detect` 之上增加结构化来源（Claude hooks、`~/.claude/projects/*.jsonl`、
  `~/.codex/sessions/*.jsonl`），启发式规则保留为兜底。
- **新的 Agent CLI**：在配置里加 `[agents.<name>]` 即可，无需改代码。
- **语音引擎**：替换 `voice.command`；整理模型替换 `voice.organizer`。
- **多端协议**：`duolo-core` 的模型类型只增不改，作为以后守护进程与客户端协议的基础。

## 多语言

界面文案在 `app/src/i18n`，菜单文案在 `duolo-app/src/locale.rs`，后端错误以 `UserError` 错误码交给界面翻译。详见 [i18n.md](i18n.md)。

## 后台审阅和内置语音

- 审阅：`duolo-engine/src/review.rs` 在任务的 worktree 里运行 `review_command`（默认 `codex exec --sandbox read-only`），
  结论保存在 `Task.review`，界面一键交给写代码的 Agent。
- 语音：`duolo-app/dictate/main.swift` 用 macOS Speech 框架识别，`build.rs` 编译后作为 externalBin 打进应用；
  输出 `~ 临时结果` / `= 最终结果` 行，`duolo-voice::Recorder` 读取。

## 约定

- 错误用 `anyhow`，面向用户的提示用中文，说清楚原因和怎么修。
- 不允许 `unsafe`，唯一例外是 `duolo-voice` 里给子进程发 SIGINT，带 SAFETY 注释。
- 提交前：`cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`。
