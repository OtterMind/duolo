import Foundation

/// 消息格式的版本，和 crates/duolo-core/src/wire.rs 一致。
public let wireVersion = 1

/// 让电脑做的事，和 crates/duolo-core/src/event.rs 的 `Command` 一致。
public enum Command: Codable, Hashable, Sendable {
    case createTask(project: String, prompt: String, base: String?, writer: String?, reviewer: String?)
    case sendMessage(task: Int64, text: String)
    case answer(task: Int64, option: Int, prompt: Int64)
    case interrupt(task: Int64)
    case startReview(task: Int64)
    case closeTask(task: Int64)

    /// 只对当下有意义：电脑不在线就不发，也不排队，晚到的"允许"可能批准另一件事。
    public var isLive: Bool {
        switch self {
        case .answer, .interrupt: true
        default: false
        }
    }

    public var task: Int64? {
        switch self {
        case .createTask: nil
        case .sendMessage(let task, _), .answer(let task, _, _), .interrupt(let task), .startReview(let task), .closeTask(let task): task
        }
    }

    private enum Keys: String, CodingKey {
        case type, project, prompt, repo, base, branch, writer, reviewer, task, text, option
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        func task() throws -> Int64 { try c.decode(Int64.self, forKey: .task) }
        switch try c.decode(String.self, forKey: .type) {
        case "create_task":
            self = .createTask(
                project: try c.decode(String.self, forKey: .project), prompt: try c.decode(String.self, forKey: .prompt),
                base: try c.decodeIfPresent(String.self, forKey: .base), writer: try c.decodeIfPresent(String.self, forKey: .writer),
                reviewer: try c.decodeIfPresent(String.self, forKey: .reviewer))
        case "send_message": self = .sendMessage(task: try task(), text: try c.decode(String.self, forKey: .text))
        case "answer":
            self = .answer(task: try task(), option: try c.decode(Int.self, forKey: .option), prompt: try c.decode(Int64.self, forKey: .prompt))
        case "interrupt": self = .interrupt(task: try task())
        case "start_review": self = .startReview(task: try task())
        case "close_task": self = .closeTask(task: try task())
        case let type:
            throw DecodingError.dataCorruptedError(forKey: .type, in: c, debugDescription: "unknown command \(type)")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .createTask(let project, let prompt, let base, let writer, let reviewer):
            try c.encode("create_task", forKey: .type)
            try c.encode(project, forKey: .project)
            try c.encode(prompt, forKey: .prompt)
            // 仓库和分支名由电脑按默认值定。
            try c.encodeNil(forKey: .repo)
            try c.encodeNil(forKey: .branch)
            try c.encode(base, forKey: .base)
            try c.encode(writer, forKey: .writer)
            try c.encode(reviewer, forKey: .reviewer)
        case .sendMessage(let task, let text):
            try c.encode("send_message", forKey: .type)
            try c.encode(task, forKey: .task)
            try c.encode(text, forKey: .text)
        case .answer(let task, let option, let prompt):
            try c.encode("answer", forKey: .type)
            try c.encode(task, forKey: .task)
            try c.encode(option, forKey: .option)
            try c.encode(prompt, forKey: .prompt)
        case .interrupt(let task):
            try c.encode("interrupt", forKey: .type)
            try c.encode(task, forKey: .task)
        case .startReview(let task):
            try c.encode("start_review", forKey: .type)
            try c.encode(task, forKey: .task)
        case .closeTask(let task):
            try c.encode("close_task", forKey: .type)
            try c.encode(task, forKey: .task)
        }
    }
}

public struct CommandResult: Codable, Hashable, Sendable {
    public var ok: Bool
    public var task: Int64?
    /// 错误码，文案是 `error.<code>`。
    public var error: String?
}

/// 手机发给电脑的。
public enum PhoneMessage: Codable, Hashable, Sendable {
    case sync
    case fetch(task: Int64, after: Int64)
    case command(id: String, at: Int64, command: Command)

    private enum Keys: String, CodingKey { case v, type, task, after, id, at, command }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "sync": self = .sync
        case "fetch": self = .fetch(task: try c.decode(Int64.self, forKey: .task), after: try c.decode(Int64.self, forKey: .after))
        case "command":
            self = .command(
                id: try c.decode(String.self, forKey: .id), at: try c.decode(Int64.self, forKey: .at),
                command: try c.decode(Command.self, forKey: .command))
        case let type:
            throw DecodingError.dataCorruptedError(forKey: .type, in: c, debugDescription: "unknown message \(type)")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        try c.encode(wireVersion, forKey: .v)
        switch self {
        case .sync: try c.encode("sync", forKey: .type)
        case .fetch(let task, let after):
            try c.encode("fetch", forKey: .type)
            try c.encode(task, forKey: .task)
            try c.encode(after, forKey: .after)
        case .command(let id, let at, let command):
            try c.encode("command", forKey: .type)
            try c.encode(id, forKey: .id)
            try c.encode(at, forKey: .at)
            try c.encode(command, forKey: .command)
        }
    }
}

/// 电脑发给手机的。
public enum DesktopMessage: Decodable, Hashable, Sendable {
    case overview(Overview)
    case events(task: Int64, events: [TaskEvent], latest: Int64)
    case review(task: Int64, finishedAt: Int64, output: String)
    case result(id: String, result: CommandResult)
    /// 这个版本还不认识的消息，忽略。
    case unknown

    private enum Keys: String, CodingKey { case type, task, events, latest, finishedAt, output, id, result }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "overview": self = .overview(try Overview(from: decoder))
        case "events":
            self = .events(
                task: try c.decode(Int64.self, forKey: .task), events: try c.decode([TaskEvent].self, forKey: .events),
                latest: try c.decode(Int64.self, forKey: .latest))
        case "review":
            self = .review(
                task: try c.decode(Int64.self, forKey: .task), finishedAt: try c.decode(Int64.self, forKey: .finishedAt),
                output: try c.decode(String.self, forKey: .output))
        case "result": self = .result(id: try c.decode(String.self, forKey: .id), result: try c.decode(CommandResult.self, forKey: .result))
        default: self = .unknown
        }
    }
}
