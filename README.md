# Duolo

本地优先的 AI 开发搭档。在一个终端里并行运行多个 Claude Code 和 Codex：每个任务一个独立的 git worktree，默认 Claude 写、Codex 审，所有 Agent 的状态在侧边栏一眼看清，用语音说需求也能直接填进 CLI。

```
┌ duolo ─────────────────────────┐ ◐ Claude · 写   ○ Codex · 审          duolo/3-stripe  +48 −6
│                                │
│ ▾ shop                 2 个任务 │  (Claude Code 原生界面，按键直接交给它)
│   ● 订单导出 CSV            2m │
│       ● Codex 写 · 等你确认  2m │
│   ◐ Stripe 订阅支付        26m │
│       ◐ Claude 写 · 运行中  26m │
│       ○ Codex 审 · 等待输入  3m │  > 金额全部改用整数分，年付打八折      ↵ 发送  Esc 还原
│                                │
│ 3 个运行中 · 1 个等你           │ ~/.duolo/wt/shop/3-stripe                          ⌃] 菜单
└────────────────────────────────┘
```

## 第一版能做什么

- **多任务并行**：每个任务自动创建分支 `duolo/<id>-<slug>` 和独立 worktree，互不干扰。
- **原生 CLI 嵌入**：右侧就是 Claude Code / Codex 本身，Duolo 自己做终端模拟和画面合成，不改变它们的任何行为。
- **侧边栏看状态**：项目 → 任务 → Agent 三层，每个 Agent 显示 运行中 / 等你确认 / 等待输入 / 已结束。
- **一写一审**：一键启动只读的 Codex 审阅当前任务相对基准分支的全部改动。
- **语音输入**：`⌃Space` 开始和结束，识别结果直接显示在 Agent 的输入行上，可选自动整理，`↵` 发送。
- **会话恢复**：重新打开 Duolo 后，选中任务按 `Enter` 即可用 `claude --continue` / `codex resume --last` 接着干。

## 安装

需要 Rust 1.86+、git，以及至少一个 Agent CLI（`claude` 或 `codex`）。

```sh
cargo install --path crates/duolo-cli
duolo doctor
```

## 使用

```sh
cd ~/code/shop
duolo                          # 打开工作台，当前仓库自动加入项目
duolo new "订单导出 CSV，支持按日期筛选"   # 不打开界面直接建任务，下次打开时自动启动
duolo ls                       # 列出任务
duolo add ~/code/api           # 添加项目
```

### 按键

在 Agent 窗格里，所有按键都直接交给 Agent（包括 Claude 自己的 `/` 命令）。Duolo 只占用两个键：

| 按键 | 作用 |
|---|---|
| `⌃]` 然后 `j` / `k` | 切换任务 |
| `⌃]` 然后 `h` / `l` | 切换同一任务里的 Agent |
| `⌃]` 然后 `1`–`9` | 跳到第几个任务 |
| `⌃]` 然后 `n` | 新任务 |
| `⌃]` 然后 `r` | 启动审阅 Agent（只读） |
| `⌃]` 然后 `s` | 启动或恢复当前 Agent |
| `⌃]` 然后 `x` | 关闭任务（删除 worktree，保留分支） |
| `⌃]` 然后 `b` / `?` / `q` | 侧边栏 / 帮助 / 退出 |
| `⌃]` `⌃]` | 把 `⌃]` 本身发给 Agent |
| `⌃Space` | 语音输入，再按一次结束 |
| `⇧PgUp` / `⇧PgDn` | 查看历史输出 |

语音文字出现后：`↵` 发送，`Tab` 只填入不发送，`Esc` 还原成原话（再按一次放弃）。

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

识别引擎通过外部命令接入，方便按平台和隐私要求替换。仓库里的 `scripts/voice-whisper.sh` 用 sox 录音、whisper.cpp 本地转写，音频不离开电脑：

```sh
brew install sox whisper-cpp
mkdir -p ~/.duolo/models
curl -L -o ~/.duolo/models/ggml-large-v3-turbo.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
```

终端需要允许麦克风权限（macOS：系统设置 → 隐私与安全性 → 麦克风，勾选你的终端 App）。

## 架构

见 [docs/architecture.md](docs/architecture.md)。简要来说：

```
duolo-cli ─▶ duolo-tui ─▶ duolo-engine ─▶ duolo-pty   (PTY + VT 终端模拟 + 状态识别)
                 │              ├───────▶ duolo-git   (worktree、改动统计)
                 └▶ duolo-voice └───────▶ duolo-core  (模型、配置、持久化)
```

## 第一版的限制

- 没有守护进程：退出 Duolo 会结束所有 Agent，重新打开后按 `Enter` 恢复会话。
- 状态识别是启发式的（看输出节奏和确认提示），还没有接入 hooks 和会话日志。
- 语音是"说完再出字"，还不能边说边出字。
- 暂不支持 Windows；手机端、审阅页、自动化、云端服务在后续版本。
