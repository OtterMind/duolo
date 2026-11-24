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
            case .message(fromUser: false, let text):
                for (i, part) in text.components(separatedBy: "`").enumerated() where i % 2 == 1 { push(part) }
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
}
