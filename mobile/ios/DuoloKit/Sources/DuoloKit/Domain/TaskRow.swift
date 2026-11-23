import Foundation

/// 列表上的一行。规则和 Android 一致，样例在 mobile/contract/behavior.json。
public struct TaskRow: Hashable, Sendable, Identifiable {
    public enum Group: String, Sendable, CaseIterable {
        case attention, active, done

        var order: Int { Self.allCases.firstIndex(of: self)! }
    }

    public enum State: Hashable, Sendable {
        case needsYou, issues(Int), reviewFailed, crashed
        case writing, reviewing
        case passed, reviewed, written, ended, notStarted

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
        state = Self.state(task)
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

    static func state(_ task: TaskOverview) -> State {
        if task.status == "needs_input" { return .needsYou }
        if task.reviewingSince != nil { return .reviewing }
        if task.status == "running" { return .writing }
        if task.status == "failed" { return .crashed }
        // 审阅之后又接着做了：以前的结论不算数。
        if let review = task.review, !(task.status == "idle" && task.since > review.finishedAt) {
            if !review.ok { return .reviewFailed }
            switch review.verdict {
            case "issues": return .issues(review.issues)
            case "pass": return .passed
            default: return .reviewed
            }
        }
        switch task.status {
        case "idle": return .written
        case "exited": return .ended
        default: return .notStarted
        }
    }

    public static func rows(_ tasks: [TaskOverview]) -> [TaskRow] {
        tasks.map(TaskRow.init).sorted { a, b in
            if a.group != b.group { return a.group.order < b.group.order }
            if a.task.updatedAt != b.task.updatedAt { return a.task.updatedAt > b.task.updatedAt }
            return a.id > b.id
        }
    }
}
