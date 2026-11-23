import Foundation

/// 任务详情里的内容，由事件推导。规则和 Android 一致，样例在 mobile/contract/behavior.json。
public struct Timeline: Hashable, Sendable {
    public enum StepKind: String, Sendable { case read, edit, run, search, other }

    public struct Step: Hashable, Sendable {
        public let seq: Int64
        public let kind: StepKind
        public let tool: String
        public let target: String
    }

    public enum Item: Hashable, Sendable, Identifiable {
        case user(seq: Int64, text: String)
        case agent(seq: Int64, text: String)
        /// 两句话之间的工具调用合成一组。
        case steps([Step])
        case review(seq: Int64, reviewer: String, ok: Bool, verdict: String, issues: Int, summary: String)

        public var id: Int64 {
            switch self {
            case .user(let seq, _), .agent(let seq, _), .review(let seq, _, _, _, _, _): seq
            case .steps(let steps): steps.first?.seq ?? 0
            }
        }
    }

    public private(set) var items: [Item] = []
    /// 最后一次工具调用；之后说过话就没有了。
    public private(set) var activity: Step?
    public private(set) var diff: DiffOverview?
    public private(set) var closed = false
    private var reviewer = ""

    public init(_ events: [TaskEvent] = []) {
        for event in events { add(event) }
    }

    public var steps: [Step] {
        items.flatMap { item -> [Step] in
            if case .steps(let steps) = item { return steps }
            return []
        }
    }

    public mutating func add(_ event: TaskEvent) {
        switch event.kind {
        case .message(let fromUser, let text):
            activity = nil
            items.append(fromUser ? .user(seq: event.seq, text: text) : .agent(seq: event.seq, text: text))
        case .tool(let name, let target):
            let step = Step(seq: event.seq, kind: Self.kind(of: name), tool: name, target: target)
            activity = step
            if case .steps(let steps) = items.last {
                items[items.count - 1] = .steps(steps + [step])
            } else {
                items.append(.steps([step]))
            }
        case .reviewStarted(let name):
            reviewer = name
        case .reviewFinished(let ok, let verdict, let issues, let summary):
            items.append(.review(seq: event.seq, reviewer: reviewer, ok: ok, verdict: verdict, issues: issues, summary: summary))
        case .diff(let stat):
            diff = stat
        case .taskClosed:
            closed = true
        case .taskCreated, .taskRenamed, .status, .prompt, .unknown:
            break
        }
    }

    static func kind(of tool: String) -> StepKind {
        switch tool.lowercased() {
        case "read", "notebookread", "view_image": .read
        case "edit", "multiedit", "write", "notebookedit", "apply_patch": .edit
        case "bash", "shell", "exec_command", "local_shell", "unified_exec": .run
        case "grep", "glob", "ls", "websearch", "webfetch", "web_search": .search
        default: .other
        }
    }

    /// 一组里读了、改了几个文件（同一个文件只算一次），运行了几次，其他几次。
    public struct Counts: Hashable, Sendable {
        public let read, edit, run, other: Int

        public init(_ steps: [Step]) {
            func files(_ kind: StepKind) -> Int { Set(steps.filter { $0.kind == kind }.map(\.target)).count }
            read = files(.read)
            edit = files(.edit)
            run = steps.filter { $0.kind == .run }.count
            other = steps.filter { $0.kind == .search || $0.kind == .other }.count
        }
    }
}
