<h1 align="center">Duolo</h1>

<p align="center">让 Claude 写、Codex 审，在自己的电脑上并行跑一组编程 Agent。</p>

<p align="center">
  <a href="https://github.com/OtterMind/duolo/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/OtterMind/duolo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="许可证：MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center"><a href="README.md">English</a> · 简体中文</p>

Duolo 是一个本地优先的桌面应用，支持 macOS、Windows 和 Linux。每个任务一个独立的 git worktree，里面跑的就是原生的 Agent CLI：Claude Code、Codex、OpenCode、pi 或 Grok；写完让另一个模型只读审阅，结论一键交回去修改。所有任务的状态在侧边栏一眼看清，也可以直接用语音说需求。代码和会话都留在你自己的电脑上。

> Duolo 还在早期，还没有发布安装包，目前需要从源码构建。macOS 用得最多；Windows 和 Linux 由 CI 编译和测试，还没有在真机上完整用过。

```
┌────────────────────────┬─────────────────────────────────────────────────┐
│ 🔍 搜索任务    ⌘K   ⊞  │ ✳ 订单导出 CSV ✓   feat/order-csv +48 −6   审阅 ⋯│
│                        ├─────────────────────────────────────────────────┤
│ ▾ shop               + │                                                 │
│ ✳ 订单导出 CSV      ✓  │   Claude Code 的原生界面                        │
│   order-csv +48 −6  2m │   （按键直接交给它）                            │
│ ✳ Stripe 订阅支付   ◐  │                                                 │
│ ❀ 修登录页样式      ●  │ ┌─────────────────────────────────────────────┐ │
│                        │ │ ❯ 金额全部改用整数分                     🎙 │ │
│ ⚙                      │ └─────────────────────────────────────────────┘ │
└────────────────────────┴─────────────────────────────────────────────────┘
```

## 能做什么

- **多任务并行**：每个任务一个新分支和独立的工作目录（git worktree，放在 `~/.duolo/wt/`），互不干扰，也不碰你正在用的那份代码。
- **原生 CLI 嵌入**：右侧就是 Claude Code / Codex / OpenCode / pi / Grok 本身，Duolo 不改变它们的任何行为，每个任务都可以换。
- **终端或聊天**：Claude Code、Codex 和支持 ACP 协议的 Agent（目前是 OpenCode 和 Grok）也可以在聊天模式下运行：对话、工具调用和确认按钮由 Duolo 自己画，回复边写边显示。在任务顶栏的图标上切换，接着同一个会话；新任务沿用上次的选择。
- **侧边栏看状态**：一个任务一行，左边是模型图标，右边一个状态小图标（运行中 / 等你确认 / 做完了 / 出错），审阅结果也是一个小标记；有任务等你时顶部出现提醒。搜索、折叠项目、双击改名、右键菜单。
- **一写一审**：在"交给…"里选审阅，Codex 在后台只读审阅这个任务从基准分支分出来之后的全部改动（包括还没提交的和新文件）。审阅在一份临时副本里进行，只给只读的工具：Claude 只能读文件，Codex 在它的只读沙箱里运行，连接的云端应用和插件都关掉。它做什么都碰不到你的代码。结论一键交给 Claude 修改。
- **交给另一个 Agent**：点任务顶栏的"交给…"（或者在侧边栏右键任务），选谁来、做什么：审阅改动、在同一个文件夹里接力，或者分出新任务接着做。分出的新任务把现在的代码（包括没提交的改动）复制到自己的分支，原来的 Agent 继续做原任务，两边并行。不管哪种，新的 Agent 都会拿到需求、最近的对话和你补充的一句话，并先看一遍现在的改动。一天之内换回原来的 Agent，它会接着自己原来的会话，并被告知这期间发生了什么。命令行：`duolo handoff <任务> --to codex [--fork | --review] [补充的话]`；终端界面里按 `t`。
- **并排看多个任务**：把侧边栏的任务拖到右边（或者 `⌘` 点它），最多同时看六个；放在某一格的边上就排在它旁边，放在中间就换掉它。项目名旁边的网格图标一键并排这个项目的全部任务，等你处理的排在前面。拖格子之间的线调大小（双击线恢复一样大），拖格子左上角的把手换位置，`⌘⇧↵` 放大一格，点格子右上角的 × 关掉这一格（任务不关）。`⌘\` 回到一格。
- **内置终端**：按 `⌃``（或者点任务顶栏的终端图标），在 Agent 下方打开终端，就在这个任务的文件夹里，用的是你自己的 shell；可以开多个标签，拖上面的线调高度。不会跳到软件外面（「⋯」菜单里还留着「在系统终端中打开」）。任务也可以不用 AI：新建任务时选「终端」，写一条命令（例如 `pnpm dev`、`cargo test`），不写就只开一个 shell。它在任务自己的分支里运行，侧边栏显示运行完了还是出错了，一键重新运行。命令行：`duolo new --writer shell "cargo test"`。
- **项目**：项目就是一个工作文件夹，可以是一个 git 仓库，也可以是装了几个仓库的文件夹。侧边栏顶部"添加项目"（`⌘O`）：打开现有文件夹、新建文件夹、从 GitHub 克隆，或者点这台电脑上找到的仓库；也可以把文件夹拖进窗口。新建和克隆默认放在 `~/DuoloProjects`。
- **新建任务**：点项目名后面的 +（或 `⌘N`），写一句需求回车就开始。其他都有默认值，想改再点：改项目里的哪个仓库、基于哪个分支、新分支叫什么（AI 根据需求起名，比如 `feat/order-csv-export`），以及谁写、谁审（Claude / Codex，没装的会给出安装命令）。
- **语音输入**：按住 `Fn` 说话（Windows / Linux 是 `Ctrl+Shift+Space`），松开插入（按住 Fn 再点空格可以锁定）；macOS 和 Windows 用系统自带的识别，免配置。说话时输入块变成录音条，实时出字、显示音量，没声音会提示。整理后填进 Agent 的输入框，可以换回原话或撤销。
- **启动时不停下来问**：在任务里启动 Agent 之前，Duolo 先在 Claude Code（`~/.claude.json`）和 Codex（`~/.codex/config.toml`，按仓库记）里把这个文件夹记为已信任，并关掉它们启动时的升级提示，它们就不会停下来问。Claude 的那一项在任务关闭时删掉。
- **会话恢复**：重新打开任务时按会话编号接着这个 Agent 自己的那次会话（`claude --resume <id>` / `codex resume <id>`）；没有可恢复的会话时带着原始需求重新开始。
- **不丢改动**：关闭任务时，没提交的改动先提交到任务的分支，再删除工作目录；没法完整保存（例如里面有嵌套仓库）就不关，并说明原因。被 git 忽略的文件（如 `.env`）会删除。
- **关窗口不停工**（macOS）：`⌘W` 只是收起窗口，Agent 继续在后台运行，点 Dock 图标回来；`⌘Q` 才会结束它们。Windows / Linux 上关窗口就是退出，有 Agent 在运行时会先确认。

## 安装

需要 git，以及至少一个 Agent CLI：[Claude Code](https://docs.claude.com/en/docs/claude-code)（`curl -fsSL https://claude.ai/install.sh | bash`）、[Codex](https://github.com/openai/codex)（`curl -fsSL https://chatgpt.com/codex/install.sh | sh`）、[OpenCode](https://opencode.ai)（`npm install -g opencode-ai`）、[pi](https://pi.dev)（`npm install -g @earendil-works/pi-coding-agent`）或 [Grok](https://x.ai/cli)（`curl -fsSL https://x.ai/cli/install.sh | bash`）。Duolo 也可以替你一键安装 Claude Code 和 Codex，并用你已有的 Claude 或 ChatGPT 账号登录；发现电脑上装了 Claude 或 ChatGPT 桌面版时会主动提示。从源码构建还需要 Rust 1.89+、Node 20+ 和 pnpm。

```sh
git clone https://github.com/OtterMind/duolo.git && cd duolo
cd app && pnpm install && pnpm tauri build
cargo install --path ../crates/duolo-cli        # 可选：命令行 duolo
```

打出来的安装包在 `target/release/bundle/` 下：

| 系统 | 安装包 | 说明 |
|---|---|---|
| macOS 12+ | `macos/Duolo.app` | 拷到"应用程序" |
| Windows 10 / 11 | `nsis/Duolo_*_x64-setup.exe`、`msi/*.msi` | 安装程序只给当前用户装，不需要管理员权限 |
| Linux | `deb/*.deb`、`appimage/*.AppImage` | 需要 WebKitGTK 4.1（Ubuntu 22.04+ / Debian 12+ 自带）；从源码构建先装 `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libssl-dev` |

开发时用 `cd app && pnpm tauri dev`，界面改动会热更新。

## 使用

双击 Duolo 打开，直接进入 Claude 的命令行：有任务时打开最近的任务，没有就新建一个。每个任务都在独立的 worktree 里，像平时一样在 Claude 的输入框里说需求；第一次输入的内容会自动成为任务标题，AI 再据此给分支起名。`⌘N` 再开一个任务。

命令行（可选）：

```sh
cd ~/code/shop
duolo                          # 打开 Duolo，当前仓库自动加入项目
duolo new "订单导出 CSV，支持按日期筛选"   # 不打开界面直接建任务
duolo ls                       # 列出任务
duolo handoff 3 --to codex     # 让 Codex 接手 3 号任务
duolo handoff 3 --to pi --fork "改成游标分页"  # Pi 在新任务里接着做，3 号任务照旧
duolo tui                      # 在终端里打开（适合 SSH 远程）
```

### 快捷键

下面是 macOS 的写法。Windows / Linux 上 `⌘` 换成 `Ctrl`，`⌥` 换成 `Alt`；`⌘` 加字母的再加上 `Shift`（`⌘N` → `Ctrl+Shift+N`，`⌘K` → `Ctrl+Shift+K`），因为 `Ctrl` 加字母要留给终端里的 Agent。终端里有选中文字时 `Ctrl+C` 复制，`Ctrl+V` 粘贴。界面上的提示会自动按当前系统显示。

| 按键 | 作用 |
|---|---|
| `⌘N` | 新任务（什么都不写直接回车，就开一个空白任务） |
| `⌘K` | 搜索任务 |
| `⌥↓` | 跳到下一个等你确认的任务 |
| `⌘O` | 添加项目 |
| `⌘⇧R` | 让 Codex 审阅当前任务 |
| `⌃Tab` | 在当前任务的 Agent 之间切换 |
| `⌘1`–`⌘9`、`⌘⌥↑↓` | 切换任务 |
| `⌘\` | 在旁边再放一个任务 / 回到一格 |
| `⌃`` | 打开或收起任务下方的终端 |
| `⌘⌥←→` / `⌘⇧↵` | 并排时切到上一格、下一格 / 放大当前这一格 |
| `⌘⇧F` | 在 Finder（文件资源管理器）中打开工作目录 |
| `⌘⇧W` | 关闭任务（没提交的改动先提交到分支，再删除 worktree；分支保留） |
| 按住 `Fn` | 语音输入，松开插入；按住时点空格锁定，再按 Fn 结束（也可以点输入块右边的麦克风，或 `⌘⇧Space`）。Windows / Linux 上按住 `Ctrl+Shift+Space`，轻按一下开始、再按一下结束 |
| 录音中 `↵` / `⌘↵` / `Esc` | 插入 / 插入并发送 / 丢弃 |
| `⇧↵` | 在 Agent 输入框里换行 |
| `⌘W` / `⌘Q` | 收起窗口（Agent 继续运行，只有 macOS）/ 退出 |

按住 Fn 需要一次"辅助功能"授权（在左下角设置里点"去授权"）。如果系统里按 🌐 键会弹出表情或切换输入法，请在 系统设置 → 键盘 里把"按下 🌐 键时"改成"不执行任何操作"，设置里也会提示。

## 配置

`~/.duolo/config.toml`，所有字段都可省略：

```toml
writer = "claude"          # 默认写代码的 Agent
reviewer = "codex"         # 默认审阅的 Agent

[agents.claude]            # 在内置的设置上改，没写的保持不变
command = "/opt/homebrew/bin/claude"
args = ["--model", "opus"]

server = "https://duolo.example.com"  # Duolo 服务器（语音识别），写成空的表示不用

[voice]
organize = false   # 让服务器整理口头语和改口（默认不整理）
```

### 接入别的 Agent

每种 Agent 都是一个插件：一份说明怎么启动、恢复、审阅它的 `agent.toml`，放在 `~/.duolo/agents/<id>/`。写法见 [docs/agents.md](docs/agents.md)。Agent 升级后 Duolo 怎么还能用（状态报告、不让启动对话框出现、版本检查、钩子）见 [docs/agent-compat.md](docs/agent-compat.md)。

### 语音

语音用 Duolo 服务器识别（服务端转给豆包的流式语音识别），macOS、Windows、Linux 都一样：

- 录系统当前的输入设备，戴耳机时用耳机的麦克风。第一次使用时系统会请求麦克风权限。
- 开始录音时带上当前任务的热词（最近改过的文件、Agent 提到的代码名字、分支名、项目名）和最近的对话，
  中英混说、代码里的名字更容易认对。
- 不用注册：第一次使用时自动在服务器上领一个匿名身份，存在 `~/.duolo/account.json`；服务器按身份每天限额。
- 声音和这些提示会发给 Duolo 服务器和识别服务；服务器连不上时语音输入不可用，会提示原因。

## 多语言

界面、菜单、系统授权弹窗、错误提示、命令行和终端界面都支持 14 种语言，默认跟随系统，可以在侧边栏左下角的设置里切换；命令行和终端界面跟着一起变。规范见 [docs/i18n.md](docs/i18n.md)，新功能都要按它支持多语言。

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

- 没有独立的守护进程：退出会结束所有 Agent，重新打开任务时自动恢复会话。
- 状态识别是启发式的（看输出节奏和确认提示），还没有接入 hooks 和会话日志。
- Windows 和 Linux 版还没有在真机上完整测过。
- 还没有查看 diff、合并、提 PR 的界面，也没有系统通知和自动更新；iOS 和 Android 原生应用正在做（[docs/mobile.md](docs/mobile.md)），服务端在 [server/](server/README.zh-CN.md)，桌面应用还没有接上。
- Linux 上按住说话的全局快捷键只在 X11 下有效，Wayland 下请点麦克风按钮。

## 参与开发

欢迎提 issue 和 PR。提交前请运行：

```sh
(cd app && pnpm build)          # 类型检查和语言包检查
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
(cd server && pnpm check && pnpm test)   # 改了服务端时，需要先 docker compose up -d
```

约定见 [AGENTS.md](AGENTS.md)：所有给用户看的文字都要有 14 种语言，桌面应用要同时支持三个系统。

## 许可证

[MIT](LICENSE)。Claude、Claude Code 是 Anthropic 的商标，Codex 是 OpenAI 的商标；Duolo 是独立的项目，和这两家公司没有隶属关系。
