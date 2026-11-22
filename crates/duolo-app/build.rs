use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_dictate();
    localize_info_plist();
    tauri_build::build();
}

/// 语言代码和 macOS 的 `.lproj` 目录名。与 Info.plist 的 CFBundleLocalizations 一致。
const LPROJ: &[(&str, &str)] = &[
    ("en", "en"),
    ("zh-CN", "zh-Hans"),
    ("zh-TW", "zh-Hant"),
    ("ja", "ja"),
    ("ko", "ko"),
    ("es", "es"),
    ("fr", "fr"),
    ("de", "de"),
    ("pt-BR", "pt-BR"),
    ("ru", "ru"),
    ("it", "it"),
    ("tr", "tr"),
    ("vi", "vi"),
    ("id", "id"),
];

/// Info.plist 里给用户看的文字（麦克风、语音识别的授权说明）按语言写进 `<lang>.lproj/InfoPlist.strings`，
/// 文案来自仓库根目录的 `locales/*.json`（`macos.*`）。生成到 `gen/lproj/`，由 tauri.macos.conf.json 的
/// `bundle.resources` 打进应用包。
fn localize_info_plist() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let locales = manifest.join("../../locales");
    for (code, lproj) in LPROJ {
        let file = locales.join(format!("{code}.json"));
        println!("cargo:rerun-if-changed={}", file.display());
        let text = std::fs::read_to_string(&file).expect("read locale");
        let messages: std::collections::HashMap<String, String> = serde_json::from_str(&text).expect("locale JSON");
        let entry = |plist_key: &str, key: &str| {
            let value = messages.get(key).unwrap_or_else(|| panic!("{code}.json is missing {key}"));
            format!("\"{plist_key}\" = \"{}\";\n", value.replace('\\', "\\\\").replace('"', "\\\""))
        };
        let strings = entry("NSMicrophoneUsageDescription", "macos.microphoneUsage")
            + &entry("NSSpeechRecognitionUsageDescription", "macos.speechUsage");
        let dir = manifest.join("gen/lproj").join(format!("{lproj}.lproj"));
        std::fs::create_dir_all(&dir).expect("create lproj dir");
        std::fs::write(dir.join("InfoPlist.strings"), strings).expect("write InfoPlist.strings");
    }
}

/// 在 macOS 上用 swiftc 编译语音识别小程序 `duolo-dictate`，放到 `binaries/` 下，
/// 由 Tauri 作为 externalBin 打进应用包（文件名要带目标三元组，见 tauri.macos.conf.json）。
fn build_dictate() {
    println!("cargo:rerun-if-changed=dictate/main.swift");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("apple-darwin") {
        // Windows 用随应用安装的 dictate/dictate.ps1，Linux 由用户配置 voice.command。
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = manifest.join("binaries").join(format!("duolo-dictate-{target}"));
    std::fs::create_dir_all(out.parent().expect("binaries dir")).expect("create binaries dir");

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
