# Duolo

本地优先的 AI 开发搭档，一个 Mac 桌面应用。并行运行多个 Claude Code 和 Codex：每个任务一个独立的 git worktree，默认 Claude 写、Codex 审，所有 Agent 的状态在侧边栏一眼看清，用语音说需求也能直接发给 Agent。

```
┌──────────────────────┬──────────────────────────────────────────────────────────┐
│ ● ● ●                │ Stripe 订阅支付  [◐ Claude 写] [○ Codex 审]   duolo/3-stripe +48 −6 ⋯│
│ + 新任务         ⌘N  ├──────────────────────────────────────────────────────────┤
│                      │                                                          │
│ shop                 │   Claude Code 原生界面（xterm.js 渲染，按键直接交给它）    │
│ ● 订单导出 CSV    2m │                                                          │
│     ● Codex 写 等你确认                                                         │
│ ◐ Stripe 订阅支付 26m│                                                          │
│     ◐ Claude 写 运行中│ ┌ 金额全部改用整数分，年付打八折 ─────────────────────┐ │
│     ○ Codex 审 等待输入│ │ 已整理                     ↵ 发送  Tab 只填入  Esc 看原话 │ │
│ + 添加项目           │ └──────────────────────────────────────────────────────┘ │
│ 1 个运行中 · 1 个等你 │                                                          │
└──────────────────────┴──────────────────────────────────────────────────────────┘
```

## 能做什么

- **多任务并行**：每个任务自动创建分支 `duolo/<id>-<slug>` 和独立 worktree，互不干扰。
- **原生 CLI 嵌入**：右侧就是 Claude Code / Codex 本身，Duolo 不改变它们的任何行为。
- **侧边栏看状态**：项目 → 任务 → Agent 三层，每个 Agent 显示 运行中 / 等你确认 / 等待输入 / 已结束。
- **一写一审**：点"审阅"，Codex 在后台只读审阅当前任务的全部改动，结论一键交给 Claude 修改。
- **语音输入**：`⌘⇧Space` 或点麦克风，用 macOS 自带识别，免配置；说话时实时出字，整理后填进 Agent 的输入框。
- **会话恢复**：重新打开任务时自动接着上次的会话（`claude --continue` / `codex resume --last`）；没有可恢复的会话时带着原始需求重新开始。
- **关窗口不停工**：`⌘W` 只是收起窗口，Agent 继续在后台运行，点 Dock 图标回来；`⌘Q` 才会结束它们。

## 安装

需要 git，以及至少一个 Agent CLI（`claude` 或 `codex`）。从源码构建还需要 Rust 1.86+、Node 20+ 和 pnpm。

```sh
cd app && pnpm install && pnpm tauri build      # 生成 target/release/bundle/macos/Duolo.app
cp -R ../target/release/bundle/macos/Duolo.app /Applications/
cargo install --path ../crates/duolo-cli        # 可选：命令行 duolo
```

开发时用 `cd app && pnpm tauri dev`，界面改动会热更新。

## 使用

双击 Duolo 打开，直接进入 Claude 的命令行：有任务时打开最近的任务，没有就新建一个。每个任务都在独立的 worktree 里，像平时一样在 Claude 的输入框里说需求；第一次输入的内容会自动成为任务标题和分支名。`⌘N` 再开一个任务。

命令行（可选）：

```sh
cd ~/code/shop
duolo                          # 打开 Duolo，当前仓库自动加入项目
duolo new "订单导出 CSV，支持按日期筛选"   # 不打开界面直接建任务
duolo ls                       # 列出任务
duolo tui                      # 在终端里打开（适合 SSH 远程）
```

### 快捷键

| 按键 | 作用 |
|---|---|
| `⌘N` | 新任务（直接打开新的命令行） |
| `⌘O` | 添加项目 |
| `⌘⇧R` | 让 Codex 审阅当前任务 |
| `⌃Tab` | 在当前任务的 Agent 之间切换 |
| `⌘1`–`⌘9`、`⌘⌥↑↓` | 切换任务 |
| `⌘⇧F` | 在 Finder 中显示工作目录 |
| `⌘⇧W` | 关闭任务（删除 worktree，保留分支） |
| `⌘⇧Space` | 语音输入，再按一次结束（也可以点输入行右边的麦克风） |
| `⇧↵` | 在 Agent 输入框里换行 |
| `⌘W` / `⌘Q` | 收起窗口（Agent 继续运行）/ 退出 |

说话时识别的文字实时显示在麦克风旁边；结束后自动整理，填进 Agent 的输入框，看一眼按 `↵` 发送。

## 配置

`~/.duolo/config.toml`，所有字段都可省略：

```toml
writer = "claude"          # 默认写代码的 Agent
reviewer = "codex"         # 默认审阅的 Agent

[agents.claude]
command = "claude"
resume_args = ["--continue"]

[agents.codex]
command = "codex"
resume_args = ["resume", "--last"]
review_args = ["--sandbox", "read-only"]

[voice]
# 录音并转写的命令：启动时开始录音，收到 SIGINT 后把文字打印到 stdout
command = ["/path/to/duolo/scripts/voice-whisper.sh"]
organize = true                                   # 整理口头语和改口
organizer = ["claude", "-p", "--model", "haiku"]  # 整理用的命令
```

### 语音

默认使用 macOS 自带的语音识别（Speech 框架，能在本机识别时不上传音频），不需要任何配置；第一次使用时系统会请求麦克风和语音识别权限。识别语言跟随界面语言。

也可以换成自己的识别命令。识别引擎通过外部命令接入，方便按平台和隐私要求替换。仓库里的 `scripts/voice-whisper.sh` 用 sox 录音、whisper.cpp 本地转写，音频不离开电脑：

```sh
brew install sox whisper-cpp
mkdir -p ~/.duolo/models
curl -L -o ~/.duolo/models/ggml-large-v3-turbo.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
```

第一次录音时 macOS 会请求麦克风权限。语音没配置时，也可以直接用 macOS 听写（连按两下 Fn）。

## 多语言

界面、菜单和错误提示支持 14 种语言，默认跟随系统，可以在侧边栏左下角的地球图标里切换。规范见 [docs/i18n.md](docs/i18n.md)，新功能都要按它支持多语言。

## 架构

见 [docs/architecture.md](docs/architecture.md)。简要来说：

```
app/（React + xterm.js 界面）
   │ Tauri 命令 / 事件 / Channel
duolo-app ─┐
duolo-tui ─┴▶ duolo-engine ─▶ duolo-pty   (PTY + 输出订阅 + 状态识别)
     │              ├───────▶ duolo-git   (worktree、改动统计)
     └▶ duolo-voice └───────▶ duolo-core  (模型、配置、持久化)
```

## 第一版的限制

- 没有独立的守护进程：`⌘Q` 退出会结束所有 Agent，重新打开任务时自动恢复会话。
- 状态识别是启发式的（看输出节奏和确认提示），还没有接入 hooks 和会话日志。
- 语音是"说完再出字"，还不能边说边出字。
- 暂时只支持 macOS；手机端、审阅页、自动化、云端服务在后续版本。
