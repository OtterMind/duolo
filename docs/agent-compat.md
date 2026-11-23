# Agent 升级后还能用：版本适配的做法

目标：Claude Code、Codex 这些 Agent 升级后，Duolo 不用大改就能继续用；最坏的情况只改插件的声明
（`crates/duolo-agent/agents/<id>/agent.toml`，写法见 [agents.md](agents.md)），不改代码。

一句话：**不依赖 Agent 的画面长什么样，依赖它官方提供的接口；接口做不到的，提前避免，而不是事后去认。**

## 升级时会坏在哪里

都是真遇到过的：

| 会变的 | 例子 | 后果 |
|---|---|---|
| 画面上的字 | "esc to interrupt"、"1. Yes"、"enter to confirm" | 认错状态，在对话框上替用户按回车 |
| 新冒出来的对话框 | Codex 启动时的"有新版本"，默认选项是"现在升级" | 发消息时的回车把 Agent 升级了 |
| 信任文件夹的提示 | 每个新任务的目录都问一次 | 任务卡在启动上 |
| 参数 | 新版加的选项旧版不认（`unknown option`） | 启动失败 |
| 无界面运行时的工具 | Codex 新加的云端连接（部署网站、改设置……）不受只读沙箱管 | "只读审阅"不再只读 |
| 会话记录 | 文件放在哪里、路径是不是解析过链接 | 读不到对话、恢复错会话 |

## 参考：Orca 是怎么做的

[Orca](https://github.com/stablyai/orca)（MIT 开源，看的是 2026-11 的 1.4.256 和当时的源码）支持几十种 Agent，做法：

1. **状态从结构化的渠道来，有先后**：
   - 钩子优先：Agent 官方的钩子（会话开始、提交需求、调用工具、请求授权、结束……）。
     钩子脚本只有几行，把 Agent 给的原始 JSON 用 curl 发到 Orca 的本地 HTTP 服务（带令牌），解析全在 Orca 里；
     Orca 没开时先存进文件，之后补发。钩子先检查 Orca 的环境变量，平时单独用 Agent 不受影响。
   - 其次是 OSC 7501（Program Status Protocol）：Agent 自己把状态写进终端输出。
   - 最后才看 Claude 窗口标题里的 `✳`。读画面只剩"现在能不能粘贴"，而且有超时。
2. **不认对话框，让它别出现**：启动前把"这个文件夹已信任"写进 Agent 的配置；关掉启动时的升级提示
   （Claude `DISABLE_AUTOUPDATER=1`，Codex `-c check_for_update_on_startup=false`）。
   它源码里的注释："在 Codex 的升级界面按回车会选'现在升级'，装完就退出。"
3. **每个能力记下从哪个版本开始有**：可选参数、钩子事件都标了最低版本，装的版本够新才用
   （旧版 Claude 遇到不认识的钩子事件会拒绝整个设置文件）。整体只设下限（Codex 0.136.0），读不出版本时照样用。
4. **从失败中学**：`unknown option '--x'` 退出时记住这个参数不支持；按"程序的真实路径 + 修改时间 + 工作目录"记，
   Agent 自己升级后自动重新判断。
5. **支持表写在文档里**：哪个 Agent 支持什么、用哪个真实版本验证过、哪天验证的。

它的代价是量很大：Claude 的钩子装在 Orca 自己的一套 Claude 配置目录里（把用户的设置同步进去），
Codex 的钩子写进用户的 `~/.codex/hooks.json`，而 Codex 的钩子要用户批准才生效，为此有一整个模块管批准记录。

## Duolo 的做法

和 Orca 同样的原则，做法尽量简单。

### 1. 状态：Agent 自己说的为准，画面兜底

| 来源 | 在哪里 | 说明 |
|---|---|---|
| OSC 7501 | `duolo-pty/src/status.rs` | 回答 Agent 的询问 `ESC ] 7501 ; ?`，之后它报告 idle / working / done / blocked / error。收到过报告就只看报告 |
| 钩子 | `duolo-core/src/hook.rs`、`duolo-agent/src/hooks.rs` | 请求授权时的工具和命令、准确的会话编号（见下面第 5 节） |
| 画面上的字 | 插件的 `[screen]` | 只在它不报告、或者还没开始报告时用（启动时的对话框期间两个 Agent 都还不报告） |

### 2. 对话框：不让它出现

- 信任文件夹：插件的 `[trust]`，启动前写进 Agent 自己的配置（`duolo-agent/src/trust.rs`）。
  Claude 按文件夹的真实路径记、子文件夹不继承，所以每个任务目录记一项，任务关闭时删掉；
  Codex 按主仓库记，在 worktree 里启动也认主仓库，记一次就够，不删（那是用户的项目）。
  用户自己说过"不信任"的不改；配置文件不在、读不懂时什么也不做（到时候照样会问）。
- 升级提示：`always_args`、`env`（config.toml 的 `args` 换不掉它们，用户改了参数也不会把它丢掉）。
- 实在避免不了的对话框：`[screen]` 的 `approval`、`choosing` 写全，Duolo 在这时不发消息（`Engine::submit` 排队）。

### 3. 版本

- `minimum_version`：更旧的在环境检查和选 Agent 的菜单里显示"版本太旧"，不能选；读不出版本时照样用。
- 版本号由环境检查问一次（`--version`），按程序的真实路径和修改时间记住（`duolo_core::process::probe`），
  启动 Agent 时只看记住的，不拿着锁去运行它。
- 钩子事件按版本装（见第 5 节）。
- 从失败中学：定了会话编号却马上因为不认得那个选项退出的，记住这种 Agent 不定编号，重新开始（`Engine::retry_failed_resumes`）。

### 4. 只读审阅：只给允许的

- Claude：`--restricted`（不读用户和项目的设置、去掉运行代码的工具）+ `--strict-mcp-config` + 只给 Read、Grep、Glob。
- Codex：系统强制的只读沙箱；功能用白名单——先运行 `codex features list`，开着的、不在 `allow` 里的一律关掉
  （插件的 `[review.features]`，`duolo_core::process::FeatureGate`）。以后新加的功能默认关着。
  代价：Codex 改了运行命令要用的功能名时，审阅会读不了文件（坏得明显，不会悄悄变得不安全），把新名字补进 `allow` 就好。
- 审阅在独立的副本里（`git clone --shared`，没有远程），改动由 Duolo 写进 `DUOLO_REVIEW.md`，审阅者不用运行 git。

### 5. 钩子（简化版）

和 Orca 比，省掉了 HTTP 服务、令牌、curl、各系统的脚本、单独的配置目录和 Codex 的批准记录：

- **只装 Claude 的**。Codex 已经报告 OSC 7501，对话记录也读得到；它的钩子要用户批准，暂时不碰。
- **不改用户的配置**：每次启动生成一份设置文件 `~/.duolo/hooks/<任务>-<Agent>.settings.json`，用 `--settings` 交给它。
- **钩子命令就是 Duolo 自己的程序**：`"<Duolo 程序>" __hook`。它把标准输入里的事件写成
  `~/.duolo/hooks/<任务>-<Agent>/<纳秒>-<进程号>.json`（先写临时文件再改名），引擎读了就删。
  桌面应用和 `duolo` 命令行都认这个参数（`main` 的第一件事），Windows 上不用 shell 脚本。
- **只要两个事件**，每个写明最早的版本，读不出版本时不装：
  - `SessionStart`（1.0.62）：会话编号，清空对话（`/clear`）后的新会话也跟得上；比从会话记录里认的准。
  - `PermissionRequest`（2.0.45）：用什么工具、运行什么命令，记成 `Approval` 事件（手机上显示）。
- 还没做：在手机上直接批准或拒绝（要钩子等 Duolo 的回答，得有同步的通道）。现在回答还是往终端里按数字键。

### 6. 会话编号：定 > 钩子 > 认

1. 能定的由 Duolo 定（Claude `--session-id <uuid>`，插件的 `session_id_args`）。
2. 钩子说的（`SessionStart`）直接记下。
3. 都没有的从会话记录里认（`duolo-agent/src/transcript/`）：同一个目录里别的 Agent 已经认领的不算，
   换到别的文件要连续两次都看到它。

## 现在每种 Agent 用了什么

最后验证：2026-11-23，Claude Code 2.1.311、Codex 0.169.1，macOS。

| | Claude Code | Codex |
|---|---|---|
| 状态 | OSC 7501（2.1.295 起） | OSC 7501（0.163 起；空闲时不一定马上报） |
| 会话编号 | Duolo 定（`--session-id`）+ 钩子 | 从会话记录里认（`payload.id`），按编号恢复 |
| 信任文件夹 | `~/.claude.json`（或 `$CLAUDE_CONFIG_DIR/.claude.json`），每个任务目录 | `~/.codex/config.toml`（或 `$CODEX_HOME`），主仓库 |
| 升级提示 | `DISABLE_AUTOUPDATER=1` | `-c check_for_update_on_startup=false` |
| 只读审阅 | `--restricted`、`--strict-mcp-config`、只给读文件的工具 | 只读沙箱、关掉 MCP 和联网搜索、功能白名单 |
| 钩子 | `SessionStart`、`PermissionRequest` | 不用 |
| 最低版本 | 不设（各功能有退路） | 0.136.0 |

## Agent 升级后怎么检查

1. `cargo test -p duolo-engine real_commands -- --ignored --nocapture`：用本机真实的 CLI 试一遍审阅和起名的参数。
2. 打开 Duolo 新建一个任务：不该停在任何对话框上，侧边栏的状态跟得上（运行中 / 等你确认 / 空闲）。
3. 审阅一次：结论里能看到具体的文件和行号（说明读到了改动）。Codex 的话，读不了文件多半是功能名变了，看 `codex features list`。
4. 关一个任务：`~/.claude.json` 里这个任务目录的那一项没了。
5. 有变化的话改插件的声明，更新上面这张表和日期。
