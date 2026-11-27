import Foundation

// 列表页需要的全部内容，和 crates/duolo-core/src/wire.rs 一致。

public struct AgentOption: Codable, Hashable, Sendable {
    public var kind: String
    public var name: String
    /// 图标的颜色（插件的 `icon_color`，`#rrggbb`）；没有时用文字的颜色。
    public var color: String?

    public init(kind: String, name: String, color: String? = nil) {
        self.kind = kind
        self.name = name
        self.color = color
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
    /// 列表上显示的状态，电脑按 crates/duolo-core/src/wire.rs 的 `TaskState` 算好；不认识的当作未启动。
    public var state: String
    /// 改的是项目里的哪个仓库；项目本身就是仓库时为空。
    public var repo: String
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
        state = try c.value(.state, or: "not_started")
        repo = try c.value(.repo, or: "")
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
    /// 电脑上的语音设置，手机说话时照着用。
    public var voice: VoiceOverview

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        rev = try c.decode(Int64.self, forKey: .rev)
        projects = try c.value(.projects, or: [])
        tasks = try c.value(.tasks, or: [])
        agents = try c.value(.agents, or: [])
        writer = try c.value(.writer, or: "")
        reviewer = try c.value(.reviewer, or: "")
        voice = try c.value(.voice, or: VoiceOverview())
    }
}

/// 语音设置：要认准的词、说完后是否整理成指令（电脑端设置 → 语音）。
public struct VoiceOverview: Codable, Hashable, Sendable {
    public var organize = false
    public var words: [String] = []

    public init() {}

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        organize = try c.value(.organize, or: false)
        words = try c.value(.words, or: [])
    }
}
