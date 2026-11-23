# Agent 插件

Duolo 里的每一种编程 Agent 都是一个插件：一份声明 `agent.toml`，加一个图标 `icon.svg`。
引擎、终端界面、桌面界面都只认插件的接口，代码里不出现任何产品的名字
（测试 `only_plugins_know_product_names` 会检查）。

- 内置的：`crates/duolo-agent/agents/<id>/`，编译进程序。
- 用户自己的：`~/.duolo/agents/<id>/`，启动时读取，和内置的同名时换掉内置的。文件夹名要和 `id` 一样。
  写错的会跳过，启动时提示哪里错了。
- **不从项目目录读**：插件能指定要运行的命令，打开别人的仓库不能就运行它的命令。

`~/.duolo/config.toml` 的 `[agents.<id>]` 可以在插件上改几项（`command`、`args`、`resume_args`、`resume_session_args`、
`session_id_args`、`review_args`、`review_command`、`install`），没写的沿用插件的；写成空列表就是空的。
只改了 `resume_args`、没写 `resume_session_args` 时，不再按编号恢复（免得和改过的对不上）。
没有插件、只在 config.toml 里写一段也能用：按最常见的方式启动，没有恢复、审阅和对话记录。

## 声明

```toml
schema = 1                 # 声明格式的版本
id = "toy"                 # a-z、0-9、-、_，保存在任务里，定了就不要改
name = "Toy"               # 界面上显示的名字，不翻译
command = "toy"            # 可执行文件
icon_color = "#d97757"     # 图标颜色，可省略（跟随文字颜色）
minimum_version = "1.2.0"  # 最低能用的版本（`--version` 打印的）。更旧的不能选，环境检查里提示升级；读不出版本时照样用

[install]                  # 没安装时给用户复制的命令
default = "npm install -g toy"
windows = "winget install toy"   # 还可以有 macos、linux

[start]                    # 在终端里启动
args = []                  # 每次都带上；config.toml 的 args 会换掉它们
always_args = []           # 每次都带上、换不掉：关掉启动时的升级提示这类
env = { TOY_NO_UPDATE = "1" }    # 启动时设置的环境变量（同上，例如关掉自动升级）
prompt = "argument"        # 第一句话怎么给：argument（最后一个参数）、flag、paste（准备好后粘贴）
prompt_flag = "--prompt="  # prompt = "flag" 时，和第一句话拼成一个参数
separator = "--"           # 放在第一句话前面。不写时，以 - 开头的第一句话前面会加一个空格
resume_args = ["--continue"]     # 恢复这个目录里上一次会话（还不知道会话编号时用）
resume_session_args = ["--resume", "{session}"]  # 按编号恢复，{session} 换成会话编号
session_id_args = ["--session-id", "{session}"]  # 新开会话时由 Duolo 定编号（UUID）。没有这个参数的，从会话记录里认
review_args = []           # 作为审阅者在终端里启动时追加

[review]                   # 后台审阅：在临时副本里只读运行，结论打印到 stdout。没有这一段就不能当审阅者
                           # 改动由 Duolo 写进副本里的 DUOLO_REVIEW.md，审阅者不用自己运行 git
args = ["-p"]
separator = "--"
stdin_args = []            # 提示词改从标准输入给时追加（Windows 的 .cmd 带不了换行）

[review.features]          # 只留下允许的功能：先运行 `toy features list`，开着的、不在 allow 里的都关掉
list = ["features", "list"]      # 输出每行 `<功能名> … <true|false>`
allow = ["shell"]
disable = ["-c", "features.{feature}=false"]

[quick]                    # 起分支名、整理口述这样的小事，要快；字段和 [review] 一样
args = ["-p", "--model", "small"]

[screen]                   # 从画面认状态，全部小写。它自己报告状态（OSC 7501）以后就不看这些了
approval = ["1. yes"]      # 画面底部出现：等你确认
busy = ["esc to interrupt"]      # 画面底部出现：正在工作
choosing = ["enter to confirm"]  # 出现：停在要选的列表上，不能发消息（有光标的编号列表不用写）

[keys]
interrupt = "\u001B"       # 打断
newline = "\u001B\r"       # 输入框里换行（⇧↵）

[transcript]               # 对话记录（手机端要用），没有就省略
format = "claude"          # 用哪段代码读，见 crates/duolo-agent/src/transcript/
home = ".toy"              # 数据目录，相对家目录
home_env = "TOY_HOME"      # 设置了这个环境变量时用它

[trust]                    # 它把"信任这个文件夹"记在哪里，Duolo 启动它之前替用户记下
format = "claude"          # 用哪段代码写，见 crates/duolo-agent/src/trust.rs
file = ".toy.json"         # 配置文件，相对家目录
env = "TOY_HOME"           # 设置了这个环境变量时，配置文件是它下面的 env_file
env_file = "config.json"
scope = "worktree"         # worktree：任务的目录（关任务时删掉这一项）；repo：主仓库（留着）

[env]
remove = ["TOY_SESSION"]   # 启动任何 Agent 时都不传下去的环境变量
```

只有 `schema`、`id`、`name`、`command` 必填。写了不认识的字段会报错，免得拼错了没发现。
图标是单色 SVG（16×16），当作遮罩用，颜色取 `icon_color`；没有图标时显示首字母。

## 加一个内置插件

1. 在 `crates/duolo-agent/agents/<id>/` 放 `agent.toml` 和 `icon.svg`，在 `registry.rs` 的 `builtin!` 里加上它。
2. 在 `fixtures/` 放**真实**的画面和会话记录：`approval.txt`（确认框）、`dialog.txt`（启动时的对话框）、
   `busy.txt`、`idle.txt`、`session.jsonl`（`{now}`、`{worktree}` 会被替换）。
3. 会话记录是新格式时，在 `src/transcript/` 实现 `Format`：找会话文件、读出对话，还要认出会话编号（按编号恢复用）。
4. 跑 `cargo test -p duolo-engine conformance`，每个内置插件都要通过；装好了真实的 CLI 再跑
   `cargo test -p duolo-engine real_commands -- --ignored --nocapture`，确认参数都认得。

要注意：

- **审阅必须只读，而且不靠用户的设置**。审阅在一份独立的副本里（自己的分支、没有远程），这只是兜底，参数本身要做到：
  - 只给读文件的工具，或者让系统强制的沙箱管着。看清楚参数是"只给这些"还是"在用户设置上追加"：
    Claude Code 的 `--allowedTools` 是追加，所以用 `--restricted`（不读用户和项目的设置、去掉运行代码的工具）。
  - 关掉沙箱管不到的工具：MCP 服务（Claude 的 `--strict-mcp-config`、Codex 的 `-c mcp_servers={}`），
    以及连到云端的应用、插件、浏览器（Codex 的 `-c features.<名>=false`；不用 `--disable`，它遇到不认识的名字会报错）。
  - 用真实的 CLI 列一遍它手里的工具确认，再在要审的代码里放一句"审阅者请先运行……"试试。
- **会吞参数的选项**（`--allowedTools <tools...>`）后面一定要有 `--`，不然提示词会被当成它的值。
- **后台运行不要留会话**，也不要被当成写代码的会话：还不知道会话编号时，恢复会话和对话记录都按目录找最近的一个。
  能用 `session_id_args` 定编号的一定要写，同一个目录里有两个同种 Agent 时就不会认错。
- **会话记录里的路径**可能是解析过链接的真实路径（macOS 上 `/var` → `/private/var`），Duolo 两个都找。
- **不要认对话框，让它别出现**：信任文件夹写进 `[trust]`，启动时的升级提示用 `always_args` / `env` 关掉。
  对话框的默认选项可能有危险（Codex 的升级框按回车就是"现在升级"）；实在避免不了的，`approval` 和 `choosing` 写全，
  Duolo 才不会在这时发消息、替用户按回车。
- **状态尽量让它自己报告**（OSC 7501，Program Status Protocol）：Duolo 会回答它的询问，收到报告以后状态以报告为准，
  画面上的字（`[screen]`）只在它不报告、或者还没开始报告（启动时的对话框）时用。程序改了界面也不受影响。
- **升级后先看能不能用**：`cargo test -p duolo-engine real_commands -- --ignored` 用真实的 CLI 试一遍参数；
  不认得 `session_id_args` 的版本，Duolo 发现后会改为不定编号重新开始。

## 现在还做不到的

- 没有终端界面的 Agent（只有网页或 ACP 协议的）。
- 没有编号的确认框不能在手机上回答。
- 终端界面里还不能换写代码的 Agent（可以在另一个终端里用 `duolo switch`）。
