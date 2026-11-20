# 给 AI Agent 和贡献者的说明

- 架构和依赖方向见 docs/architecture.md：`core` 不依赖任何 crate，`engine` 不依赖 UI。
- 新功能先想清楚放在哪一层；和进程、终端相关的放 `duolo-pty`，和任务生命周期相关的放 `duolo-engine`。
- 持久化的模型字段只增不改，新增字段带 `#[serde(default)]`。
- 面向用户的文案用中文，说清楚发生了什么、怎么处理。
- 提交前运行：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --workspace`。
