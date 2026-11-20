# 给 AI Agent 和贡献者的说明

- 架构和依赖方向见 docs/architecture.md：`core` 不依赖任何 crate，`engine` 不依赖 UI。
- 新功能先想清楚放在哪一层；和进程、终端相关的放 `duolo-pty`，和任务生命周期相关的放 `duolo-engine`。
- 持久化的模型字段只增不改，新增字段带 `#[serde(default)]`。
- **所有面向用户的功能都必须支持多语言**（14 种，见 docs/i18n.md）：界面文字一律通过 `t()` 取，
  先在 `app/src/i18n/locales/en.ts` 加键，再补齐所有语言；后端给用户看的错误用 `UserError` 错误码，
  不返回写死的句子；原生菜单文案在 `crates/duolo-app/src/locale.rs`。
- 文案说清楚发生了什么、怎么处理。代码注释用中文，日志和命令行输出用英文。
- 桌面界面在 `app/`，只通过 `src/lib/api.ts` 调用后端；后端命令在 `crates/duolo-app/src/commands.rs`，只做参数转换。
  发给界面的数据结构在 `view.rs`，与 `api.ts` 的类型保持一致。
- 提交前运行：`cd app && pnpm build`（类型检查），以及 `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --workspace`。
