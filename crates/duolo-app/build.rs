use std::path::PathBuf;

fn main() {
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

/// Info.plist 里给用户看的文字（麦克风的授权说明）按语言写进 `<lang>.lproj/InfoPlist.strings`，
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
        let strings = entry("NSMicrophoneUsageDescription", "macos.microphoneUsage");
        let dir = manifest.join("gen/lproj").join(format!("{lproj}.lproj"));
        std::fs::create_dir_all(&dir).expect("create lproj dir");
        std::fs::write(dir.join("InfoPlist.strings"), strings).expect("write InfoPlist.strings");
    }
}
