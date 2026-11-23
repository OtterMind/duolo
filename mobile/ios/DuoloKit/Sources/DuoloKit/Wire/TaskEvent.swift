import Foundation

/// 任务里发生的一件事，和 crates/duolo-core/src/event.rs 一致。
public struct TaskEvent: Codable, Hashable, Sendable {
    public var task: Int64
    /// 任务内从 1 开始连续递增。
    public var seq: Int64
    /// Unix 毫秒。
    public var at: Int64
    public var kind: Kind

    public enum Kind: Hashable, Sendable {
        case taskCreated(title: String, branch: String)
        case taskRenamed(title: String, branch: String)
        case taskClosed
        case message(fromUser: Bool, text: String)
        case tool(name: String, target: String)
        case status(String)
        case prompt(question: String, detail: String, options: [String])
        case reviewStarted(reviewer: String)
        case reviewFinished(ok: Bool, verdict: String, issues: Int, summary: String)
        case diff(DiffOverview)
        /// 更新版本的电脑发来的、这个版本还不认识的事件，或者字段对不上的。
        case unknown
    }

    /// 收到时的原样。保存的是它，所以升级之后能重新读出以前不认识的事件。
    public let raw: JSONValue

    private enum Keys: String, CodingKey {
        case task, seq, at, type, title, branch, speaker, text, tool, target, status, question, detail, options
        case reviewer, ok, verdict, issues, summary, files, insertions, deletions
    }

    public init(from decoder: Decoder) throws {
        raw = try JSONValue(from: decoder)
        let c = try decoder.container(keyedBy: Keys.self)
        task = try c.decode(Int64.self, forKey: .task)
        seq = try c.decode(Int64.self, forKey: .seq)
        at = try c.value(.at, or: 0)
        kind = (try? Self.kind(c)) ?? .unknown
    }

    public func encode(to encoder: Encoder) throws {
        try raw.encode(to: encoder)
    }

    private static func kind(_ c: KeyedDecodingContainer<Keys>) throws -> Kind {
        func text(_ key: Keys) throws -> String { try c.decode(String.self, forKey: key) }
        switch try text(.type) {
        case "task_created": return .taskCreated(title: try text(.title), branch: try text(.branch))
        case "task_renamed": return .taskRenamed(title: try text(.title), branch: try text(.branch))
        case "task_closed": return .taskClosed
        case "message": return .message(fromUser: try text(.speaker) == "user", text: try text(.text))
        case "tool": return .tool(name: try text(.tool), target: try text(.target))
        case "status": return .status(try text(.status))
        case "prompt":
            return .prompt(question: try text(.question), detail: try text(.detail), options: try c.decode([String].self, forKey: .options))
        case "review_started": return .reviewStarted(reviewer: try text(.reviewer))
        case "review_finished":
            return .reviewFinished(
                ok: try c.decode(Bool.self, forKey: .ok), verdict: try text(.verdict),
                issues: try c.decode(Int.self, forKey: .issues), summary: try text(.summary))
        case "diff":
            return .diff(DiffOverview(
                files: try c.decode(Int.self, forKey: .files), insertions: try c.decode(Int.self, forKey: .insertions),
                deletions: try c.decode(Int.self, forKey: .deletions)))
        default: return .unknown
        }
    }
}
