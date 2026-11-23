import Foundation

// 列表页需要的全部内容，和 crates/duolo-core/src/wire.rs 一致。

public struct AgentOption: Codable, Hashable, Sendable {
    public var kind: String
    public var name: String

    public init(kind: String, name: String) {
        self.kind = kind
        self.name = name
    }
}

public struct ProjectOverview: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var branch: String
    public var branches: [String]

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        branch = try c.value(.branch, or: "")
        branches = try c.value(.branches, or: [])
    }
}

public struct PromptOverview: Codable, Hashable, Sendable {
    /// 这条提示的事件编号，回答时带回去。
    public var seq: Int64
    public var question: String
    public var detail: String
    public var options: [String]

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        seq = try c.decode(Int64.self, forKey: .seq)
        question = try c.decode(String.self, forKey: .question)
        detail = try c.value(.detail, or: "")
        options = try c.decode([String].self, forKey: .options)
    }
}

public struct ReviewOverview: Codable, Hashable, Sendable {
    public var finishedAt: Int64
    public var ok: Bool
    public var verdict: String
    public var issues: Int
    public var summary: String

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        finishedAt = try c.decode(Int64.self, forKey: .finishedAt)
        ok = try c.decode(Bool.self, forKey: .ok)
        verdict = try c.decode(String.self, forKey: .verdict)
        issues = try c.value(.issues, or: 0)
        summary = try c.value(.summary, or: "")
    }
}

public struct DiffOverview: Codable, Hashable, Sendable {
    public var files: Int
    public var insertions: Int
    public var deletions: Int

    public init(files: Int, insertions: Int, deletions: Int) {
        self.files = files
        self.insertions = insertions
        self.deletions = deletions
    }
}

public struct TaskOverview: Codable, Hashable, Sendable, Identifiable {
    public var id: Int64
    public var project: String
    public var title: String
    public var untitled: Bool
    public var branch: String
    public var status: String
    public var writer: AgentOption
    public var reviewer: AgentOption
    public var updatedAt: Int64
    public var since: Int64
    public var last: String
    public var prompt: PromptOverview?
    public var reviewingSince: Int64?
    public var review: ReviewOverview?
    public var diff: DiffOverview?
    public var seq: Int64

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        project = try c.decode(String.self, forKey: .project)
        title = try c.decode(String.self, forKey: .title)
        untitled = try c.value(.untitled, or: false)
        branch = try c.value(.branch, or: "")
        status = try c.decode(String.self, forKey: .status)
        writer = try c.decode(AgentOption.self, forKey: .writer)
        reviewer = try c.decode(AgentOption.self, forKey: .reviewer)
        updatedAt = try c.decode(Int64.self, forKey: .updatedAt)
        since = try c.value(.since, or: 0)
        last = try c.value(.last, or: "")
        prompt = try c.decodeIfPresent(PromptOverview.self, forKey: .prompt)
        reviewingSince = try c.decodeIfPresent(Int64.self, forKey: .reviewingSince)
        review = try c.decodeIfPresent(ReviewOverview.self, forKey: .review)
        diff = try c.decodeIfPresent(DiffOverview.self, forKey: .diff)
        seq = try c.value(.seq, or: 0)
    }
}

public struct Overview: Codable, Hashable, Sendable {
    public var rev: Int64
    public var projects: [ProjectOverview]
    public var tasks: [TaskOverview]
    public var agents: [AgentOption]
    public var writer: String
    public var reviewer: String

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        rev = try c.decode(Int64.self, forKey: .rev)
        projects = try c.value(.projects, or: [])
        tasks = try c.value(.tasks, or: [])
        agents = try c.value(.agents, or: [])
        writer = try c.value(.writer, or: "")
        reviewer = try c.value(.reviewer, or: "")
    }
}
