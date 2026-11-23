import Foundation

/// 列表上的一行。状态由电脑算好，这里只决定第二行写什么和怎么排；规则和 Android 一致，样例在 mobile/contract/behavior.json。
public struct TaskRow: Hashable, Sendable, Identifiable {
    public enum Group: String, Sendable, CaseIterable {
        case attention, active, done

        var order: Int { Self.allCases.firstIndex(of: self)! }
    }

    public enum State: Hashable, Sendable {
        case needsYou, issues(Int), reviewFailed, crashed
        case writing, reviewing
        case passed, reviewed, written, ended, notStarted

        /// 电脑发来的状态码（`TaskState`）；不认识的（更新版本的电脑）当作未启动。
        public init(code: String, issues: Int) {
            switch code {
            case "needs_you": self = .needsYou
            case "issues": self = .issues(issues)
            case "review_failed": self = .reviewFailed
            case "crashed": self = .crashed
            case "writing": self = .writing
            case "reviewing": self = .reviewing
            case "passed": self = .passed
            case "reviewed": self = .reviewed
            case "written": self = .written
            case "ended": self = .ended
            default: self = .notStarted
            }
        }

        /// 状态码，文案是 `state.<code>`。
        public var code: String {
            switch self {
            case .needsYou: "needs_you"
            case .issues: "issues"
            case .reviewFailed: "review_failed"
            case .crashed: "crashed"
            case .writing: "writing"
            case .reviewing: "reviewing"
            case .passed: "passed"
            case .reviewed: "reviewed"
            case .written: "written"
            case .ended: "ended"
            case .notStarted: "not_started"
            }
        }

        public var group: Group {
            switch self {
            case .needsYou, .issues, .reviewFailed, .crashed: .attention
            case .writing, .reviewing: .active
            case .passed, .reviewed, .written, .ended, .notStarted: .done
            }
        }
    }

    public let task: TaskOverview
    public let state: State
    /// 第二行：最能说明现在情况的一句话。
    public let detail: String

    public var id: Int64 { task.id }
    public var group: Group { state.group }

    /// 进行中的显示做了多久，其他的显示多久以前。
    public var since: Int64? {
        switch state {
        case .reviewing: task.reviewingSince
        case .writing: task.since > 0 ? task.since : nil
        default: nil
        }
    }

    public init(_ task: TaskOverview) {
        self.task = task
        state = State(code: task.state, issues: task.review?.issues ?? 0)
        let source: String
        switch state {
        case .needsYou:
            source = task.prompt.map { $0.detail.isEmpty ? $0.question : $0.detail } ?? task.last
            // 要运行的命令照原样显示第一行，不当成 Markdown。
            if task.prompt != nil {
                detail = source.split(separator: "\n").first.map(String.init) ?? ""
                return
            }
        case .issues, .reviewFailed, .passed, .reviewed:
            source = task.review.map(\.summary).flatMap { $0.isEmpty ? nil : $0 } ?? task.last
        default:
            source = task.last
        }
        detail = Excerpt.first(source)
    }

    public static func rows(_ tasks: [TaskOverview]) -> [TaskRow] {
        tasks.map(TaskRow.init).sorted { a, b in
            if a.group != b.group { return a.group.order < b.group.order }
            if a.task.updatedAt != b.task.updatedAt { return a.task.updatedAt > b.task.updatedAt }
            return a.id > b.id
        }
    }
}
