import Foundation

/// 识别前给识别服务的提示：热词（文件名、代码里的名字）和最近的对话。
/// 和电脑端的 crates/duolo-voice/src/hints.rs 一致。
public struct VoiceHints: Hashable, Sendable {
    /// 重要的在前。
    public var hotwords: [String] = []
    /// 新的在前。
    public var context: [String] = []

    static let maxHotwords = 40
    static let maxContext = 6
    static let maxContextChars = 300
    static let maxWordChars = 40

    public init(hotwords: [String] = [], context: [String] = []) {
        self.hotwords = hotwords
        self.context = context
    }

    /// `events` 旧的在前。
    public init(project: String?, title: String?, branch: String?, events: [TaskEvent]) {
        var words: [String] = []
        func push(_ raw: String) {
            let word = raw.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\"',.:;()"))
            let shaped = !word.isEmpty && word.count <= Self.maxWordChars
            let oneWord = !word.contains(where: \.isWhitespace) && word.contains(where: \.isLetter)
            guard shaped, oneWord, words.count < Self.maxHotwords, !words.contains(word) else { return }
            words.append(word)
        }
        for event in events.reversed() {
            switch event.kind {
            case .tool(_, let target) where Self.looksLikePath(target):
                if let name = target.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last { push(String(name)) }
            case .message(let fromUser, let text):
                // Agent 用反引号标出来的最可靠，先放；再是没加反引号但一看就是代码的（双方说的都算）。
                if !fromUser {
                    for (i, part) in text.components(separatedBy: "`").enumerated() where i % 2 == 1 { push(part) }
                }
                Self.codeNames(text).forEach(push)
            default: break
            }
        }
        for word in [branch, project].compactMap({ $0 }) { push(word) }

        var dialog: [String] = []
        for event in events.reversed() {
            let text: String
            switch event.kind {
            case .prompt(let question, _, let options): text = options.isEmpty ? question : "\(question) (\(options.joined(separator: " / ")))"
            case .message(_, let said): text = said
            default: continue
            }
            let trimmed = String(text.trimmingCharacters(in: .whitespacesAndNewlines).suffix(Self.maxContextChars))
            if !trimmed.isEmpty, !dialog.contains(trimmed) { dialog.append(trimmed) }
            if dialog.count >= Self.maxContext { break }
        }
        if let title = title?.trimmingCharacters(in: .whitespaces), !title.isEmpty { dialog.append(title) }
        self.init(hotwords: words, context: dialog)
    }

    /// 工具的目标是文件路径（不是一条命令）。
    static func looksLikePath(_ target: String) -> Bool {
        !target.isEmpty && !target.contains(where: \.isWhitespace) && (target.contains("/") || target.contains("\\") || target.contains("."))
    }

    /// 文字里看着像代码的名字：`exportCsv`、`snake_case`、`orders.ts`、`duolo_core::t`。普通的英文单词不算。
    static func codeNames(_ text: String) -> [String] {
        let keep = { (c: Character) in c.isASCII && (c.isLetter || c.isNumber) || "_-./:".contains(c) }
        return text.split(whereSeparator: { !keep($0) })
            .map { String($0).trimmingCharacters(in: CharacterSet(charactersIn: "-./:")) }
            .filter(isCodeName)
    }

    static func isCodeName(_ word: String) -> Bool {
        let b = Array(word.utf8)
        guard b.count >= 3, !word.contains("://"), isAlpha(b[0]) || b[0] == UInt8(ascii: "_") else { return false }
        let camel = zip(b, b.dropFirst()).contains { isLower($0) && isUpper($1) }
        let joined = (1..<b.count - 1).contains { i in "_./:".utf8.contains(b[i]) && isAlnum(b[i - 1]) && isAlnum(b[i + 1]) }
        return camel || joined || word.contains("::")
    }

    /// 按热词改正识别结果里拆开说、或大小写不对的名字："加个 use state" → "加个 useState"。
    /// 只改由几段拼成的名字，几段之间只能隔着空格、`_`、`-`、`.`、`/`。
    public func fixNames(_ text: String) -> String {
        // 名字压成小写字母数字作比较；长的先比，免得短的抢了长的。
        let names = hotwords.enumerated().compactMap { index, word -> (key: [UInt8], name: [UInt8], parts: Int, index: Int)? in
            let key = word.utf8.filter(Self.isAlnum).map(Self.lower)
            let parts = Self.wordParts(word)
            return parts >= 2 && key.count >= 5 ? (key, Array(word.utf8), parts, index) : nil
        }.sorted { ($0.key.count, -$0.index) > ($1.key.count, -$1.index) }
        guard !names.isEmpty else { return text }
        let t = Array(text.utf8)
        var runs: [(Int, Int)] = []
        var start: Int?
        for i in 0...t.count {
            let alnum = i < t.count && Self.isAlnum(t[i])
            if alnum, start == nil {
                start = i
            } else if !alnum, let s = start {
                runs.append((s, i))
                start = nil
            }
        }
        func gapOK(_ from: Int, _ to: Int) -> Bool { t[from..<to].allSatisfy { " _-./".utf8.contains($0) } }
        let underscore = UInt8(ascii: "_")
        var out: [UInt8] = []
        var copied = 0
        var i = 0
        scan: while i < runs.count {
            // 前一个字符是 `_` 说明这段是别的名字的一部分。
            if runs[i].0 > 0, t[runs[i].0 - 1] == underscore {
                i += 1
                continue
            }
            let first = i
            for name in names {
                var joined: [UInt8] = []
                for j in first..<min(runs.count, first + name.parts + 1) {
                    if j > first, !gapOK(runs[j - 1].1, runs[j].0) { break }
                    joined += t[runs[j].0..<runs[j].1].map(Self.lower)
                    if joined.count > name.key.count || !name.key.starts(with: joined) { break }
                    let (from, to) = (runs[first].0, runs[j].1)
                    if joined == name.key, !(to < t.count && t[to] == underscore) {
                        if Array(t[from..<to]) != name.name {
                            out += t[copied..<from]
                            out += name.name
                            copied = to
                        }
                        i = j + 1
                        continue scan
                    }
                }
            }
            i += 1
        }
        out += t[copied...]
        return String(decoding: out, as: UTF8.self)
    }

    /// 名字由几段组成：`useState` 2、`export_csv_v2` 3、`orders.ts` 2。
    static func wordParts(_ word: String) -> Int {
        var parts = 0
        var prev: UInt8?
        for c in word.utf8 {
            let starts: Bool
            if !isAlnum(c) {
                starts = false
            } else if let p = prev {
                starts = !isAlnum(p) || (isLower(p) && isUpper(c))
            } else {
                starts = true
            }
            if starts { parts += 1 }
            prev = c
        }
        return parts
    }

    private static func isLower(_ b: UInt8) -> Bool { (97...122).contains(b) }
    private static func isUpper(_ b: UInt8) -> Bool { (65...90).contains(b) }
    private static func isAlpha(_ b: UInt8) -> Bool { isLower(b) || isUpper(b) }
    private static func isAlnum(_ b: UInt8) -> Bool { isAlpha(b) || (48...57).contains(b) }
    private static func lower(_ b: UInt8) -> UInt8 { isUpper(b) ? b + 32 : b }
}
