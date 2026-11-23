import Foundation

/// 多语言。文案和桌面端共用仓库根目录的 locales/*.json（规范见 docs/i18n.md），原样打包进应用。
public struct Localizer: Sendable {
    /// 支持的语言，`name` 用该语言自己的写法。与 app/src/i18n/index.tsx 的 `LOCALES` 一致。
    public static let locales: [(code: String, name: String)] = [
        ("en", "English"), ("zh-CN", "简体中文"), ("zh-TW", "繁體中文"), ("ja", "日本語"), ("ko", "한국어"),
        ("es", "Español"), ("fr", "Français"), ("de", "Deutsch"), ("pt-BR", "Português (Brasil)"), ("ru", "Русский"),
        ("it", "Italiano"), ("tr", "Türkçe"), ("vi", "Tiếng Việt"), ("id", "Bahasa Indonesia"),
    ]

    public let code: String
    private let messages: [String: String]
    private let base: [String: String]

    /// `directory` 里是 `<code>.json`。
    public init(code: String, directory: URL) {
        func load(_ code: String) -> [String: String] {
            (try? Data(contentsOf: directory.appending(path: "\(code).json")))
                .flatMap { try? JSONDecoder().decode([String: String].self, from: $0) } ?? [:]
        }
        self.code = code
        base = load("en")
        messages = code == "en" ? base : load(code)
    }

    /// 系统的日期、数字按这个语言显示。
    public var locale: Locale {
        switch code {
        case "zh-CN": Locale(identifier: "zh-Hans")
        case "zh-TW": Locale(identifier: "zh-Hant")
        default: Locale(identifier: code)
        }
    }

    /// 找不到时退回英文，再找不到返回键名。`{name}` 换成 `vars` 里的值。
    public func callAsFunction(_ key: String, _ vars: [String: String] = [:]) -> String {
        var text = messages[key] ?? base[key] ?? key
        for (name, value) in vars { text = text.replacingOccurrences(of: "{\(name)}", with: value) }
        return text
    }

    public func has(_ key: String) -> Bool { base[key] != nil }

    /// 错误码对应的说明。
    public func error(_ code: String) -> String {
        has("error.\(code)") ? self("error.\(code)") : self("error.internal", ["message": code])
    }

    /// 系统语言标签对应到支持的语言，规则与桌面端的 `match_locale` 一致。
    public static func match(_ tags: [String]) -> String {
        for raw in tags {
            let tag = raw.lowercased().replacingOccurrences(of: "_", with: "-")
            if tag.hasPrefix("zh") {
                return ["hant", "tw", "hk", "mo"].contains { tag.contains($0) } ? "zh-TW" : "zh-CN"
            }
            if tag.hasPrefix("pt") { return "pt-BR" }
            var lang = tag.split(separator: "-").first.map(String.init) ?? ""
            // 印尼语的旧代码。
            if lang == "in" { lang = "id" }
            if let hit = locales.first(where: { $0.code.lowercased() == lang }) { return hit.code }
        }
        return "en"
    }
}
