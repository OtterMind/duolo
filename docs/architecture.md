# 架构

## Crate 划分

| Crate | 职责 | 依赖 |
|---|---|---|
| `duolo-core` | 领域模型（项目、任务、Agent）、配置、`state.json` 持久化、文本工具 | 无 UI、无进程 |
| `duolo-git` | git 命令行封装：仓库定位、worktree、改动统计 | git |
| `duolo-pty` | 在 PTY 里运行 Agent CLI，VT 解析维护屏幕，按输出节奏和屏幕内容识别状态 | portable-pty、vt100 |
| `duolo-voice` | 语音：调用外部录音转写命令、调用整理命令 | core |
| `duolo-engine` | 调度：任务生命周期、worktree、Agent 启动与恢复、会话管理 | core、git、pty |
| `duolo-tui` | 终端界面：侧边栏、Agent 窗格、语音输入行、按键路由 | engine、voice、ratatui |
| `duolo-cli` | `duolo` 命令行入口 | 以上全部 |

依赖只能自上而下：`core` 不依赖任何人，`engine` 不依赖 UI。

## 数据流

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

- **守护进程 `duolod`**：把 `Engine` 搬进常驻进程，TUI 通过 Unix socket 调用同样的方法；`duolo-pty` 无需改动。
  拆分后退出终端不会结束 Agent，手机端也能连接。
- **hooks 和会话日志**：在 `duolo-pty::detect` 之上增加结构化来源（Claude hooks、`~/.claude/projects/*.jsonl`、
  `~/.codex/sessions/*.jsonl`），启发式规则保留为兜底。
- **新的 Agent CLI**：在配置里加 `[agents.<name>]` 即可，无需改代码。
- **语音引擎**：替换 `voice.command`；整理模型替换 `voice.organizer`。
- **多端协议**：`duolo-core` 的模型类型只增不改，作为以后守护进程与客户端协议的基础。

## 约定

- 错误用 `anyhow`，面向用户的提示用中文，说清楚原因和怎么修。
- 不允许 `unsafe`，唯一例外是 `duolo-voice` 里给子进程发 SIGINT，带 SAFETY 注释。
- 提交前：`cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`。
