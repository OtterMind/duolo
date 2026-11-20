use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_dictate();
    tauri_build::build();
}

/// 在 macOS 上用 swiftc 编译语音识别小程序 `duolo-dictate`，放到 `binaries/` 下，
/// 由 Tauri 作为 externalBin 打进应用包（文件名要带目标三元组）。
fn build_dictate() {
    println!("cargo:rerun-if-changed=dictate/main.swift");
    let target = std::env::var("TARGET").unwrap_or_default();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = manifest.join("binaries").join(format!("duolo-dictate-{target}"));
    std::fs::create_dir_all(out.parent().expect("binaries dir")).expect("create binaries dir");

    if !target.contains("apple-darwin") {
        // 其他平台还没有内置语音识别；放一个空文件满足 externalBin 的检查。
        if !out.exists() {
            std::fs::write(&out, b"").expect("write placeholder");
        }
        return;
    }
    let arch = if target.starts_with("aarch64") { "arm64" } else { "x86_64" };
    let status = Command::new("swiftc")
        .args(["-O", "-target", &format!("{arch}-apple-macos12"), "-o"])
        .arg(&out)
        .arg(manifest.join("dictate/main.swift"))
        .status();
    match status {
        Ok(s) if s.success() => {}
        // 没有 Xcode 命令行工具时不阻断构建，只是没有内置语音。
        _ => {
            println!("cargo:warning=swiftc failed; built-in voice input will be unavailable");
            if !out.exists() {
                std::fs::write(&out, b"").expect("write placeholder");
            }
        }
    }
}
