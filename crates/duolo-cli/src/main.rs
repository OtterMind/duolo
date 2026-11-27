//! `duolo` 命令行。命令都在库里（`duolo_cli`），桌面应用带上 `__cli` 运行时也用它（见 [`duolo_cli::CLI_ARG`]）。

fn main() -> std::process::ExitCode {
    duolo_cli::main(std::env::args_os())
}
