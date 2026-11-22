# 给 AI Agent 和贡献者的说明

- 架构和依赖方向见 docs/architecture.md：`core` 不依赖任何 crate，`engine` 不依赖 UI。
- 新功能先想清楚放在哪一层；和进程、终端相关的放 `duolo-pty`，和任务生命周期相关的放 `duolo-engine`。
- 持久化的模型字段只增不改，新增字段带 `#[serde(default)]`。
- 拿着引擎的锁不做费时间的事（git、等进程退出、网络）；要几秒的操作放在 `duolo_engine::ops`。
- 启动外部命令用 `duolo_core::process`（`command`、`Prompted`、`spawn_tree`），结束时用 `ProcessTree` / `Session::stop` 结束整棵进程树。
- 修改任务、项目状态只能通过 `Engine::update`（别的进程可能同时在改）；删除用户文件前先保存，见 docs/architecture.md 的"数据安全"一节。
- **所有给用户看的文字都必须支持多语言（14 种），默认英文**，规范见 docs/i18n.md。范围包括桌面界面、原生菜单、
  系统授权弹窗、报错、命令行输出和 `--help`、终端界面、写进数据的默认值。
  - 文案只有一份：仓库根目录的 `locales/*.json`。先加到 `locales/en.json`，再补齐其他 13 种语言。
  - 代码里不写死给用户看的句子：TypeScript 用 `t()`，Rust 用 `duolo_core::{t, tf}`；用占位符，不拼字符串。
  - 用户能处理的错误用 `UserError` 错误码（文案在 `error.*`），不返回写死的句子。
  - 测试数据和默认值用英文；只有测试多字节字符处理时才用其他语言。
  - 提交前 `cd app && pnpm build` 会检查语言包是否齐全、占位符是否一致、Rust 用到的键是否存在。
- **桌面应用支持 macOS、Windows、Linux**，见 docs/architecture.md 的"跨平台"一节。
  - 系统相关的代码集中放在 `duolo-app/src/platform.rs`（Rust）和 `app/src/lib/platform.ts`（界面）；按住说话在 `fnkey.rs`，原生菜单在 `main.rs`。
  - 不写死 `open`、`$HOME`、`/bin/sh`、unix 专属的 API；启动外部命令用 `duolo_core::process::command`，家目录用 `process::home_dir`。
  - 快捷键按 macOS 写法（`⌘⇧R`）定义和写进文案，Windows / Linux 上自动换算；判断按键用 `matches` / `modKey`，不直接看 `metaKey`。
  - 只有某个系统才有的说法（Finder、Dock、Fn、系统设置的路径）要按系统显示不同的文案。
- 文案说清楚发生了什么、怎么处理；能用图标的不加文字说明，图标按钮要有翻译过的悬停提示。代码注释用中文。
- 桌面界面在 `app/`，只通过 `src/lib/api.ts` 调用后端；后端命令在 `crates/duolo-app/src/commands.rs`，只做参数转换。
  发给界面的数据结构在 `view.rs`，与 `api.ts` 的类型保持一致。
- 提交前运行：`cd app && pnpm build`（类型检查），以及 `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --workspace`。
