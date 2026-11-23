# 架构

## Crate 划分

| Crate | 职责 | 依赖 |
|---|---|---|
| `duolo-core` | 领域模型（项目、任务、Agent）、配置、`state.json` 持久化、文本工具 | 无 UI、无进程 |
| `duolo-agent` | Agent 插件：每种 Agent 怎么启动、恢复、审阅，画面怎么认，会话记录怎么读（见 [agents.md](agents.md)） | core |
| `duolo-git` | git 命令行封装：仓库定位、worktree、改动统计 | git |
| `duolo-pty` | 在 PTY 里运行 Agent CLI，VT 解析维护屏幕，按输出节奏和屏幕内容识别状态（规则由插件给） | portable-pty、vt100 |
| `duolo-voice` | 语音：调用外部录音转写命令、调用整理命令 | core |
| `duolo-engine` | 调度：任务生命周期、worktree、Agent 启动与恢复、会话管理 | core、agent、git、pty |
| `duolo-app` | 桌面应用后端（Tauri）：把 Engine 的能力作为命令提供给界面，推送状态快照和 Agent 输出 | engine、voice、tauri |
| `app/` | 桌面应用界面（React + xterm.js）：侧边栏、首页、任务页、语音 | 只通过 Tauri 命令访问后端 |
| `duolo-tui` | 终端界面（`duolo tui`，适合 SSH）：自己做 VT 合成 | engine、voice、ratatui |
| `duolo-cli` | `duolo` 命令行入口，默认打开桌面应用 | 以上全部 |

依赖只能自上而下：`core` 不依赖任何人，`engine` 不依赖 UI。

## 数据安全

- **只有一条改状态的路**：`Engine::update`。它独占 `state.json.lock`，在锁里重新读一遍 `state.json`，改完保存再放开，
  所以桌面应用、终端界面和 `duolo new` 同时运行也不会互相覆盖。闭包里只改状态，不做 git 操作，也不嵌套调用。
  界面定期调用 `Engine::sync` 读进别的进程的修改；别处关掉的任务，这里的 Agent 一并结束。
- **写文件**：先写临时文件、落盘，再改名。`state.json` 每次另存一份 `.bak`；损坏时把坏文件改名成
  `state.json.damaged-<时间>` 留着，用备份恢复。`config.toml` 写错时用默认配置，文件不动。
  这些情况放在 `Engine::take_notices` 里，桌面应用弹窗、命令行写 stderr、终端界面显示在底部。
- **关闭任务**：停 Agent → `duolo_git::save_worktree` 把没提交的改动提交到当前分支（不在任何分支上时新建 `duolo/<编号>-saved`）→
  worktree 改名成同一层的 `.closing-*` → 再保存一次刚好写进来的 → 删登记 → 后台删文件。任何一步失败，任务和目录都留着：
  里面有嵌套仓库，或者子模块有改动、没推送的提交、暂存（git 存不下它们的内容）时报 `nestedRepo`；文件一直在变时报 `stillChanging`。
  `assume-unchanged` / `skip-worktree` 藏起来的改动、rebase 自动暂存的改动也会保存。被 git 忽略的文件会删除，确认框里说明了。
  目录已经不是 worktree（主仓库没了）时不删，告诉用户留在哪里。

## 不卡住、不留尾巴

- **引擎的锁只在改状态时拿**：界面的按键、250ms 一次的快照都要这把锁。建 worktree、关任务这类要几秒的事放在
  `duolo_engine::ops`，分成"准备（拿锁）→ 干活（不拿锁）→ 登记（拿锁）"；改动统计在后台线程算（`diffs.rs`），
  调用方只取最近一次的结果。写终端由 `duolo-pty` 的写线程做，Agent 不读输入也不会卡住调用方。
- **结束进程就结束整棵树**：`duolo_core::proctree`。npm 装的 `codex` 外面包着一层 node，Claude Code 给每条命令开新的会话，
  只结束主进程会留下真正干活的。macOS / Linux 按父进程、会话、进程组一起找；Windows 用作业对象，有窗口的程序
  （Agent 打开的编辑器、浏览器）不动。后台命令用 `process::spawn_tree` 或 `process::Prompted` 启动；终端里的用 `Session::stop`。
- **带提示词的命令**用 `process::Prompted`：提示词一般是最后一个参数；Windows 的 `.cmd` 不能带换行，改从标准输入给。
- **替用户发消息**用 `Engine::submit`，不直接粘贴加回车：Agent 停在对话框上（信任目录、是否升级、是否允许）时，
  回车会变成替用户选。这时先排队，等画出输入框（打开括号粘贴）、没有在等用户选什么之后才发。
- **改动从哪里算**：`duolo_git::fork_point`，任务从基准分支分出来（或最近一次合并它）的提交；统计和审阅都相对它，包括新文件。
- **退出**：`request_quit` 请界面处理，3 秒没有回应（界面崩溃、卡死）就用系统弹窗问。
- **环境变量**：macOS / Linux 启动时从登录 shell 读一遍（`shell_env.rs`），PATH 用 shell 的，其他的只补没有的。

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
2. 屏幕底部出现插件声明的确认文字（`[screen] approval`）→ 等你确认；
3. 出现插件声明的忙碌文字（`[screen] busy`），或最近 1.5 秒有输出 → 运行中；
4. 否则 → 等待输入。

## 扩展点

- **守护进程 `duolod`**：把 `Engine` 搬进常驻进程，桌面应用和 TUI 通过 Unix socket 调用同样的方法；`duolo-pty` 无需改动。
  拆分后退出应用不会结束 Agent，手机端也能连接。`duolo-app/src/view.rs` 的快照格式就是以后的协议基础。
- **网页端**：`app/` 的界面只依赖 `lib/api.ts` 这一层，换成 WebSocket 实现即可复用。
- **手机端**：iOS 和 Android 各用原生语言写，在 `mobile/`，见 [mobile.md](mobile.md)。
- **对话和工具调用**：`duolo-agent` 的 `Watch` 在后台线程跟着 Agent 自己写的会话记录，引擎只取结果；
  画面识别保留为兜底。以后可以再加 Claude hooks 这样的结构化来源。
- **新的 Agent CLI**：写一个插件，见 [agents.md](agents.md)。
- **语音引擎**：替换 `voice.command`；整理的命令默认用装了的 Agent（插件的 `[quick]`），可以换成 `voice.organizer`。
- **多端协议**：`duolo-core` 的模型类型只增不改，作为以后守护进程与客户端协议的基础。

## 多语言

所有文案只有一份，在仓库根目录的 `locales/*.json`：桌面界面用 `t()`，菜单、命令行和终端界面用 `duolo_core::{t, tf}`，后端错误以 `UserError` 错误码交给界面翻译。详见 [i18n.md](i18n.md)。

## 后台审阅和内置语音

- 审阅：`duolo-engine/src/review.rs` 先用 `duolo_git::snapshot` 把任务现在的样子（包括没提交的改动和新文件）
  检出到临时副本 `~/.duolo/review/<开始时间>-<任务>-<序号>`：一个 `git clone --shared`（对象借用原仓库，
  分支、配置是自己的，去掉了远程，不跑钩子），再把从分出来的提交开始的改动写进副本里的 `DUOLO_REVIEW.md`
  （`duolo_git::changes_report`），审阅者不用自己运行 git。然后在副本里运行审阅 Agent 的命令（插件的 `[review]`），
  结束后删掉副本；没删掉的下次启动时清理。
  只读靠审阅命令本身（只给读文件的工具或系统沙箱，关掉 MCP 和云端连接，见 agents.md），副本再兜一层：
  在里面改文件、删分支都碰不到原仓库；审阅期间写代码的 Agent 继续改也不影响。
  结论里的副本路径换回任务的 worktree，保存在 `Task.review`，界面一键交给写代码的 Agent。
  10 万个文件的仓库，检出副本要 8 秒左右（审阅本身要一两分钟）。
- 换 Agent：`duolo-engine/src/switch.rs`。换写代码的：原来的记进 `Task.retired`（事件里的编号还对得上），
  连同离开时的事件编号和代码快照（`AgentSpec.left`，`duolo_git::snapshot_commit`，不拿着引擎的锁做）；
  新的 `AgentSpec` 用新编号、带着交接说明（`AgentSpec.prompt`：需求、最近 20 条对话、看 `git diff` 的提示）。
  一天之内换回原来的那种 Agent 时接回它原来的会话，恢复后先发 `AgentSpec.resume_note`（这期间的对话、对照快照看改动）；
  接不回来就带着交接说明重新开始。换审阅的只改 `Task.reviewer`。
- 换人只改状态，谁跑着 Agent 谁负责执行：`Engine::adopt`（每次读到新状态都会走）发现这个进程里跑着的 Agent
  已经不在任务里了，就停掉它，没发出去的消息交给接手的。所以命令行（`duolo switch`）、手机都能换；
  接手的由打开任务的界面启动（手机发来的命令直接启动）。
- Agent 升级后还能用的整体做法见 [agent-compat.md](agent-compat.md)。
- 钩子：`--settings` 交给 Agent 一份设置文件，钩子命令是 Duolo 自己的程序（`__hook`，`duolo-core/src/hook.rs`），
  事件写成 `~/.duolo/hooks/<任务>-<Agent>/` 里的文件，引擎（`live.rs` 的 `poll_hooks`）读了就删。
- Agent 的状态：先看它自己报告的（OSC 7501，`duolo-pty/src/status.rs` 回答询问、读报告），
  没报告过的（不支持，或者还停在启动时的对话框上）再看画面上的字（插件的 `[screen]`）。
  启动前替用户信任任务的目录（插件的 `[trust]`，`duolo-agent/src/trust.rs`）、关掉升级提示（`always_args`、`env`），
  启动时的对话框基本不会出现。测试里 Agent 的家目录换成临时目录（`DUOLO_AGENT_HOME`），不会改到真的配置文件。
- 会话编号：存在 `AgentSpec.session`，恢复时用插件的 `resume_session_args`。插件有 `session_id_args` 的（Claude Code），
  新开会话时由 Duolo 定好编号；没有的（Codex）从会话记录里认，同一个目录里别的 Agent 已经认领的会话不会认成它的。
- 语音：内置识别程序输出 `~ 临时结果` / `^ 音量` / `= 最终结果` 行，`duolo-voice::Recorder` 读取，
  结束录音时 macOS / Linux 发 SIGINT，Windows 往标准输入写一行 `stop`。
  - macOS：`duolo-app/dictate/main.swift`（Speech 框架），`build.rs` 编译后作为 externalBin 打进应用；
  - Windows：`duolo-app/dictate/dictate.ps1`（System.Speech），作为资源随应用安装，用 Windows PowerShell 运行；
  - Linux：没有内置识别，用户配置 `voice.command`（例如 `scripts/voice-whisper.sh`）。

## 跨平台

桌面应用支持 macOS、Windows、Linux，各系统不同的地方集中在：

- `duolo-app/src/platform.rs`：内置语音识别、在文件管理器 / 终端里打开目录、打开系统设置；
- `duolo-app/src/fnkey.rs`：按住说话。macOS 监听 Fn 键（CGEventTap），Windows / Linux 用全局快捷键 Ctrl+Shift+Space；
- `duolo-app/src/main.rs`：原生菜单只在 macOS 上用；关窗口在 macOS 上是隐藏，在 Windows / Linux 上是退出；
- `duolo-core/src/process.rs`：按 PATH / PATHEXT 找可执行文件（Windows 上 npm 装的 `claude` 是 `claude.cmd`）、家目录；
- `app/src/lib/platform.ts`：快捷键。统一按 macOS 写法定义，Windows / Linux 上 ⌘ 换成 Ctrl，⌘ 加字母再加 Shift，
  文案里的快捷键也自动换算（Rust 侧见 `duolo_core::i18n::pc_keys`）；
- 打包配置：`tauri.conf.json` 是公共部分，`tauri.{macos,windows,linux}.conf.json` 是各系统的安装包格式和资源。

## 事件流和远程命令（手机端的基础）

每个任务有一条事件流，电脑端界面、手机端和以后的云端同步都用它（格式在 `duolo-core/src/event.rs`）：

- **来源**：Agent 自己写的会话记录（Claude Code 的 `~/.claude/projects/…/*.jsonl`、Codex 的 `~/.codex/sessions/…`），
  变成"你说的话 / Agent 的回复 / 工具调用"；终端画面变成状态和"等你选"的提示（问题、说明、选项）；
  引擎自己记任务建好、改名、关闭、审阅、改动统计。见 `duolo-engine/src/live.rs`、`prompt.rs` 和 `duolo-agent/src/transcript/`。
- **编号**：每个任务从 1 递增，存在 `~/.duolo/events/<任务编号>.jsonl`。断线重连的客户端说"我有到第几条"，
  用 `events_since` 补齐。事件里只有数据（文件路径、命令、状态码），不放句子，客户端按自己的语言显示。
- **状态**要稳定 1.5 秒才记（Agent 界面重绘时会在"运行中 / 空闲"间来回跳）；"等你确认"和"退出"马上记。
- **命令**（`Command`）是别的设备能对这台电脑做的事：新建任务、发消息（Agent 没运行时先启动，准备好再发）、
  回答"等你选"（发对应的数字键）、打断、开始审阅、关闭任务。统一由 `Engine::execute` 执行，失败时返回错误码。
- 桌面应用每 250 毫秒取一次新事件，通过 `task-events` 推给界面；`task_events`、`execute` 两个命令给界面和以后的同步用。

## 服务端（server/）

所有用户共用的一套 Node 服务（TypeScript + Fastify + PostgreSQL），只做三件事：账号（better-auth 邮箱验证码登录）、
设备配对（电脑端显示二维码，手机扫码即登录同一账号）、电脑端和手机之间的消息转发（WebSocket + 离线信箱）。
它不理解消息内容：电脑端把事件流、手机把命令都加密后再交给它。详见 `server/README.zh-CN.md`。

## 约定

- 错误用 `anyhow`，面向用户的提示用中文，说清楚原因和怎么修。
- 不允许 `unsafe`，唯一例外是 `duolo-voice` 里给子进程发 SIGINT，带 SAFETY 注释。
- 提交前：`cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`。
