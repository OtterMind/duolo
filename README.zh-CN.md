<h1 align="center">Duolo</h1>

<p align="center">让 Claude 写、Codex 审，在自己的电脑上并行跑一组编程 Agent。</p>

<p align="center">
  <a href="https://github.com/OtterMind/duolo/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/OtterMind/duolo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="许可证：MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center"><a href="README.md">English</a> · 简体中文</p>

Duolo 是一个本地优先的桌面应用，支持 macOS、Windows 和 Linux。每个任务一个独立的 git worktree，里面跑的就是原生的 Claude Code 或 Codex；写完让另一个模型只读审阅，结论一键交回去修改。所有任务的状态在侧边栏一眼看清，也可以直接用语音说需求。代码和会话都留在你自己的电脑上。

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
- **原生 CLI 嵌入**：右侧就是 Claude Code / Codex 本身，Duolo 不改变它们的任何行为。
- **侧边栏看状态**：一个任务一行，左边是模型图标，右边一个状态小图标（运行中 / 等你确认 / 做完了 / 出错），审阅结果也是一个小标记；有任务等你时顶部出现提醒。搜索、折叠项目、双击改名、右键菜单。
- **一写一审**：点"审阅"，Codex 在后台只读审阅这个任务从基准分支分出来之后的全部改动（包括还没提交的和新文件），结论一键交给 Claude 修改。
- **项目**：项目就是一个工作文件夹，可以是一个 git 仓库，也可以是装了几个仓库的文件夹。侧边栏顶部"添加项目"（`⌘O`）：打开现有文件夹、新建文件夹、从 GitHub 克隆，或者点这台电脑上找到的仓库；也可以把文件夹拖进窗口。新建和克隆默认放在 `~/DuoloProjects`。
- **新建任务**：点项目名后面的 +（或 `⌘N`），写一句需求回车就开始。其他都有默认值，想改再点：改项目里的哪个仓库、基于哪个分支、新分支叫什么（AI 根据需求起名，比如 `feat/order-csv-export`），以及谁写、谁审（Claude / Codex，没装的会给出安装命令）。
- **语音输入**：按住 `Fn` 说话（Windows / Linux 是 `Ctrl+Shift+Space`），松开插入（按住 Fn 再点空格可以锁定）；macOS 和 Windows 用系统自带的识别，免配置。说话时输入块变成录音条，实时出字、显示音量，没声音会提示。整理后填进 Agent 的输入框，可以换回原话或撤销。
- **会话恢复**：重新打开任务时自动接着上次的会话（`claude --continue` / `codex resume --last`）；没有可恢复的会话时带着原始需求重新开始。
- **不丢改动**：关闭任务时，没提交的改动先提交到任务的分支，再删除工作目录；没法完整保存（例如里面有嵌套仓库）就不关，并说明原因。被 git 忽略的文件（如 `.env`）会删除。
- **关窗口不停工**（macOS）：`⌘W` 只是收起窗口，Agent 继续在后台运行，点 Dock 图标回来；`⌘Q` 才会结束它们。Windows / Linux 上关窗口就是退出，有 Agent 在运行时会先确认。

## 安装

需要 git，以及至少一个 Agent CLI：[Claude Code](https://docs.claude.com/en/docs/claude-code)（`npm install -g @anthropic-ai/claude-code`）或 [Codex](https://github.com/openai/codex)（`npm install -g @openai/codex`）。从源码构建还需要 Rust 1.89+、Node 20+ 和 pnpm。

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

[agents.claude]
command = "claude"
resume_args = ["--continue"]

[agents.codex]
command = "codex"
resume_args = ["resume", "--last"]
review_args = ["--sandbox", "read-only"]

[voice]
# 录音并转写的命令：启动时开始录音，收到停止信号后把文字打印到 stdout
# （macOS / Linux 发 SIGINT，Windows 往标准输入写一行 stop）
command = ["/path/to/duolo/scripts/voice-whisper.sh"]
organize = true                                   # 整理口头语和改口
organizer = ["claude", "-p", "--model", "haiku"]  # 整理用的命令
```

### 语音

不需要任何配置：

- **macOS**：系统自带的语音识别（Speech 框架，能在本机识别时不上传音频）。第一次使用时系统会请求麦克风和语音识别权限。
- **Windows**：系统自带的听写（System.Speech，在本机识别），随应用安装的 `dictate.ps1` 负责调用。需要在 设置 → 时间和语言 → 语音 里装好对应语言的语音识别。
- **Linux**：系统没有自带识别，需要按下面的方式配置 whisper.cpp。

识别语言默认跟随界面语言，可以在左下角设置里单独选。

也可以换成自己的识别命令。识别引擎通过外部命令接入，方便按平台和隐私要求替换。仓库里的 `scripts/voice-whisper.sh` 用 sox 录音、whisper.cpp 本地转写，音频不离开电脑：

```sh
brew install sox whisper-cpp          # Linux：用发行版的包管理器装 sox，whisper.cpp 从源码编译
mkdir -p ~/.duolo/models
curl -L -o ~/.duolo/models/ggml-large-v3-turbo.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
```

第一次录音时系统会请求麦克风权限。

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
