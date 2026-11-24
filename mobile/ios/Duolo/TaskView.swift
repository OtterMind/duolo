import DuoloKit
import SwiftUI

struct TaskView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let id: Int64

    @State private var draft = ""
    @State private var showSteps = false
    @State private var confirmClose = false
    @State private var answering = false
    @FocusState private var composing: Bool

    private var row: TaskRow? { model.row(id) }
    private var timeline: DuoloKit.Timeline { model.opened?.timeline ?? DuoloKit.Timeline() }

    var body: some View {
        let t = model.t
        VStack(spacing: 0) {
            header
            if let row {
                conversation(row)
                LinkNotice(answering: row.state == .needsYou)
                if let prompt = row.task.prompt, row.state == .needsYou {
                    PromptPanel(task: row.task, prompt: prompt, answering: $answering)
                } else {
                    if row.state == .needsYou { unreadablePrompt }
                    activity(row)
                    composer(row)
                }
            } else {
                Text(t("error.taskNotFound")).foregroundStyle(Palette.muted).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Palette.bg)
        .toolbar(.hidden, for: .navigationBar)
        .sheet(isPresented: $showSteps) {
            StepsSheet(task: row?.task, timeline: timeline).environment(model).presentationDetents([.medium, .large])
        }
        .alert(t("confirm.closeTitle", ["title": row.map { model.title($0.task) } ?? ""]), isPresented: $confirmClose) {
            Button(t("confirm.closeAction"), role: .destructive) {
                Task { if await model.submit(.closeTask(task: id)) { dismiss() } }
            }
            Button(t("common.cancel"), role: .cancel) {}
        } message: {
            Text(t("confirm.closeBody", ["branch": row?.task.branch ?? ""]))
        }
        .onAppear { model.open(id) }
        .onDisappear { model.leave(id) }
    }

    private var header: some View {
        let t = model.t
        return HStack(spacing: 4) {
            IconButton(icon: "chevron-left", label: t("mobile.back")) { dismiss() }
            Text(row.map { model.title($0.task) } ?? "").font(.app(16, .semibold)).lineLimit(1).frame(maxWidth: .infinity)
                .accessibilityAddTraits(.isHeader)
            Menu {
                Button(t("mobile.steps.title")) { showSteps = true }
                if let row, row.state != .reviewing {
                    Button(t(row.task.review == nil ? "review.start" : "review.rerun")) { Task { await model.submit(.startReview(task: id)) } }
                }
                Button(t("sidebar.copyBranch")) { UIPasteboard.general.string = row?.task.branch }
                Button(t("confirm.closeAction"), role: .destructive) { confirmClose = true }
            } label: {
                Icon("kebab-horizontal").frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .foregroundStyle(Palette.text)
            .accessibilityLabel(t("common.more"))
            .disabled(row == nil)
        }
        .padding(.horizontal, 8).padding(.vertical, 2)
    }

    private func conversation(_ row: TaskRow) -> some View {
        GeometryReader { box in
            ScrollView { messages(row).frame(minHeight: box.size.height, alignment: .top) }
                .defaultScrollAnchor(.bottom)
                .scrollDismissesKeyboard(.interactively)
        }
    }

    /// 内容不满一屏时从顶部排。
    private func messages(_ row: TaskRow) -> some View {
        LazyVStack(alignment: .leading, spacing: 18) {
            if timeline.items.isEmpty {
                ProgressView().frame(maxWidth: .infinity).padding(40).opacity(row.task.seq > 0 ? 1 : 0)
            }
            ForEach(timeline.items) { item in
                switch item {
                case .user(_, let text): Bubble(text: text)
                case .agent(_, let text): MarkdownText(text)
                case .steps(let steps): stepsLine(steps)
                case .review(let seq, let reviewer, let ok, let verdict, let issues, _):
                    // 最近一次的全文在下面，这里不重复。
                    if seq != expandedReview(row) {
                        ReviewLine(reviewer: reviewer, ok: ok, verdict: verdict, issues: issues)
                    }
                }
            }
            reviewSection(row)
            ForEach(model.pending(for: id)) { PendingBubble(item: $0) }
        }
        .padding(.horizontal, 24).padding(.vertical, 12)
    }

    /// 下面展开了全文的那次审阅。
    private func expandedReview(_ row: TaskRow) -> Int64? {
        guard case .issues = row.state, model.opened?.review != nil else { return nil }
        return timeline.items.last { if case .review = $0 { true } else { false } }?.id
    }

    private func stepsLine(_ steps: [DuoloKit.Timeline.Step]) -> some View {
        let t = model.t
        let n = DuoloKit.Timeline.Counts(steps)
        let parts = [
            n.read > 0 ? t("mobile.steps.read", ["count": "\(n.read)"]) : nil, n.edit > 0 ? t("mobile.steps.edit", ["count": "\(n.edit)"]) : nil,
            n.run > 0 ? t("mobile.steps.run", ["count": "\(n.run)"]) : nil, n.other > 0 ? t("mobile.steps.other", ["count": "\(n.other)"]) : nil,
        ].compactMap { $0 }
        return Button { showSteps = true } label: {
            HStack(spacing: 4) {
                Text(parts.joined(separator: " · ")).font(.app(14, relativeTo: .subheadline)).multilineTextAlignment(.leading)
                Icon("chevron-right", size: 14)
            }
            .foregroundStyle(Palette.muted).frame(minHeight: 44).contentShape(Rectangle())
        }
        .padding(.vertical, -10)
    }

    /// 最近一次审阅的全文，以及交回修改。
    @ViewBuilder private func reviewSection(_ row: TaskRow) -> some View {
        let t = model.t
        if case .issues = row.state, let output = model.opened?.review {
            let body = ReviewText.body(output)
            VStack(alignment: .leading, spacing: 14) {
                Palette.line.frame(height: 1)
                HStack(spacing: 10) {
                    Icon("issue-opened").foregroundStyle(Palette.bad)
                    Text(t("mobile.review.by", ["agent": row.task.reviewer.name])).font(.app(16, .semibold))
                }
                MarkdownText(body)
                Button(t("review.sendToWriter", ["agent": row.task.writer.name])) {
                    let text = t("review.handoff", ["agent": row.task.reviewer.name]) + "\n\n" + output
                    Task { await model.submit(.sendMessage(task: id, text: text)) }
                }
                .buttonStyle(PrimaryButtonStyle())
                .disabled(!model.pending(for: id).isEmpty)
            }
            .padding(.top, 6)
        }
    }

    private var unreadablePrompt: some View {
        HStack(spacing: 10) {
            StatusBadge(style: StatusStyle(.needsYou, model.t), size: 28)
            Text(model.t("mobile.prompt.unreadable")).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.wait)
            Spacer()
        }
        .padding(.horizontal, 24).padding(.vertical, 6)
    }

    /// 此刻在做什么，右边停止。
    @ViewBuilder private func activity(_ row: TaskRow) -> some View {
        let t = model.t
        if row.group == .active {
            HStack(spacing: 0) {
                Button { showSteps = true } label: {
                    HStack(spacing: 10) {
                        StatusBadge(style: StatusStyle(row.state, t), size: 28)
                        Text(activityText(row)).lineLimit(1)
                        Spacer(minLength: 4)
                        if let since = row.since {
                            TimelineView(.periodic(from: .now, by: 30)) { _ in Text(model.lasting(since: since)).monospacedDigit() }
                        }
                    }
                    .frame(minHeight: 44).contentShape(Rectangle())
                }
                if row.state == .writing {
                    Button { Task { await model.submit(.interrupt(task: id)) } } label: {
                        Icon("square-fill").frame(width: 44, height: 44).contentShape(Rectangle())
                    }
                    .accessibilityLabel(t("mobile.stop"))
                    .disabled(!model.computerOnline)
                }
            }
            .font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted)
            .padding(.leading, 24).padding(.trailing, 12)
        }
    }

    private func activityText(_ row: TaskRow) -> String {
        let t = model.t
        if row.state == .reviewing { return "\(t("state.reviewing")) · \(row.task.reviewer.name)" }
        guard let step = timeline.activity, !step.target.isEmpty else { return t("mobile.working", ["agent": row.task.writer.name]) }
        return "\(StepsSheet.label(step.kind, t)) \(step.target)"
    }

    private func composer(_ row: TaskRow) -> some View {
        Composer(text: $draft, focused: $composing) { text in
            Task { await model.submit(.sendMessage(task: id, text: text)) }
        }
        .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 8)
    }
}

/// 输入框：有字时是发送，没字时按住说话。
struct Composer: View {
    @Environment(AppModel.self) private var model
    @Binding var text: String
    var focused: FocusState<Bool>.Binding
    let send: (String) -> Void

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        let t = model.t
        HStack(alignment: .bottom, spacing: 4) {
            TextField(t("mobile.composer"), text: $text, axis: .vertical)
                .lineLimit(1...6).focused(focused).font(.app(16)).padding(.vertical, 14).padding(.leading, 20)
            if trimmed.isEmpty {
                HoldToTalk { said, edit in
                    if edit {
                        text = said
                        focused.wrappedValue = true
                    } else {
                        send(said)
                    }
                }
                .frame(width: 44, height: 52)
            } else {
                Button {
                    send(trimmed)
                    text = ""
                } label: {
                    Icon("arrow-up").foregroundStyle(Palette.onGreen).frame(width: 36, height: 36).background(Palette.green, in: .circle)
                        .frame(width: 44, height: 52)
                }
                .accessibilityLabel(t("mobile.send"))
            }
        }
        .padding(.trailing, 4)
        .background(Palette.raised, in: .rect(cornerRadius: 26))
    }
}

struct Bubble: View {
    let text: String

    var body: some View {
        Text(text).textSelection(.enabled).padding(.horizontal, 16).padding(.vertical, 10)
            .background(Palette.bubble, in: .rect(cornerRadius: 20))
            .frame(maxWidth: .infinity, alignment: .trailing).padding(.leading, 48)
    }
}

/// 还没送到、或没成功的话。
struct PendingBubble: View {
    @Environment(AppModel.self) private var model
    let item: OutboxItem

    var body: some View {
        let t = model.t
        let failed = item.state == .failed
        VStack(alignment: .trailing, spacing: 6) {
            if case .sendMessage(_, let text) = item.command {
                Text(text).foregroundStyle(Palette.muted).padding(.horizontal, 16).padding(.vertical, 10)
                    .overlay(RoundedRectangle(cornerRadius: 20).stroke(failed ? Palette.bad : Palette.dim, style: .init(lineWidth: 1, dash: [4, 3])))
            }
            Text(failed ? "\(t("mobile.notDelivered")) · \(t.error(item.error ?? "internal"))" : t("mobile.pending"))
                .font(.app(13, relativeTo: .footnote)).foregroundStyle(failed ? Palette.bad : Palette.muted).multilineTextAlignment(.trailing)
            if failed {
                HStack(spacing: 16) {
                    Button(t("common.cancel")) { model.discard(item) }
                    Button(t("mobile.retry")) { model.retry(item) }
                }
                .font(.app(14, .medium, relativeTo: .subheadline)).frame(minHeight: 44)
            }
        }
        .frame(maxWidth: .infinity, alignment: .trailing).padding(.leading, 48)
    }
}

struct ReviewLine: View {
    @Environment(AppModel.self) private var model
    let reviewer: String
    let ok: Bool
    let verdict: String
    let issues: Int

    var body: some View {
        let state: TaskRow.State = !ok ? .reviewFailed : verdict == "issues" ? .issues(issues) : verdict == "pass" ? .passed : .reviewed
        let style = StatusStyle(state, model.t)
        let name = model.overview?.agents.first { $0.kind == reviewer }?.name ?? reviewer.capitalized
        HStack(spacing: 8) {
            StatusBadge(style: style, size: 16).accessibilityHidden(true)
            Text("\(name) · \(style.label)")
        }
        .font(.app(14, .medium, relativeTo: .subheadline)).foregroundStyle(style.color)
        .accessibilityElement(children: .combine)
    }
}

/// Agent 在等你选：选项代替输入框，来自 Agent，原样显示。
struct PromptPanel: View {
    @Environment(AppModel.self) private var model
    let task: TaskOverview
    let prompt: PromptOverview
    @Binding var answering: Bool

    var body: some View {
        let t = model.t
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                StatusBadge(style: StatusStyle(.needsYou, t), size: 28)
                Text(t("mobile.prompt.title", ["agent": task.writer.name])).font(.app(14, .medium, relativeTo: .subheadline))
                    .foregroundStyle(Palette.wait)
            }
            VStack(alignment: .leading, spacing: 6) {
                if !prompt.detail.isEmpty {
                    ScrollView {
                        Text(prompt.detail).font(.code(15)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(maxHeight: 180).fixedSize(horizontal: false, vertical: true)
                }
                Text(prompt.question).font(.app(prompt.detail.isEmpty ? 16 : 14)).foregroundStyle(prompt.detail.isEmpty ? Palette.text : Palette.muted)
            }
            VStack(spacing: 8) {
                ForEach(Array(prompt.options.enumerated()), id: \.offset) { i, option in
                    if i == 0 {
                        Button(option) { answer(i) }.buttonStyle(PrimaryButtonStyle())
                    } else {
                        Button(option) { answer(i) }.buttonStyle(SecondaryButtonStyle())
                    }
                }
            }
            .disabled(answering || !model.computerOnline)
        }
        .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 12)
        .background(Palette.raised, in: UnevenRoundedRectangle(topLeadingRadius: 24, topTrailingRadius: 24))
        .background(Palette.raised.ignoresSafeArea(edges: .bottom).padding(.top, 40))
    }

    private func answer(_ option: Int) {
        answering = true
        Task {
            await model.submit(.answer(task: task.id, option: option, prompt: prompt.seq))
            answering = false
        }
    }
}

struct StepsSheet: View {
    @Environment(AppModel.self) private var model
    let task: TaskOverview?
    let timeline: DuoloKit.Timeline

    static func label(_ kind: DuoloKit.Timeline.StepKind, _ t: Localizer) -> String {
        switch kind {
        case .read: t("mobile.step.read")
        case .edit: t("mobile.step.edit")
        case .run: t("mobile.step.run")
        case .search: t("mobile.step.search")
        case .other: t("mobile.step.other")
        }
    }

    var body: some View {
        let t = model.t
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                Text(t("mobile.steps.title")).font(.app(18, .semibold)).accessibilityAddTraits(.isHeader).padding(.bottom, 16)
                if let diff = timeline.diff ?? task?.diff {
                    HStack(spacing: 8) {
                        Text(t("mobile.steps.files", ["count": "\(diff.files)"]))
                        Text("+\(diff.insertions)").foregroundStyle(Palette.greenText)
                        Text("−\(diff.deletions)").foregroundStyle(Palette.bad)
                    }
                    .monospacedDigit()
                }
                if let branch = task?.branch, !branch.isEmpty {
                    Text(branch).font(.code(13)).foregroundStyle(Palette.muted).textSelection(.enabled).padding(.top, 4)
                }
                Spacer().frame(height: 12)
                ForEach(timeline.steps, id: \.seq) { step in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(Self.label(step.kind, t)).foregroundStyle(Palette.muted).frame(width: 64, alignment: .leading).lineLimit(1)
                            .minimumScaleFactor(0.7)
                        Text(step.target.isEmpty || step.target.hasPrefix("***") ? step.tool : step.target).font(.code(14)).lineLimit(2)
                        Spacer(minLength: 0)
                    }
                    .font(.app(14, relativeTo: .subheadline)).frame(minHeight: 40)
                    .accessibilityElement(children: .combine)
                }
            }
            .padding(24)
        }
        .background(Palette.bg)
        .foregroundStyle(Palette.text)
    }
}

enum ReviewText {
    /// 审阅全文去掉第一行的判断标记（`[ISSUES 2] …`）。
    static func body(_ output: String) -> String {
        let text = output.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.hasPrefix("["), let end = text.firstIndex(of: "]") else { return text }
        return text[text.index(after: end)...].trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// Agent 的话：段落、标题、列表、代码块。只做常见的格式，其余原样显示。
struct MarkdownText: View {
    private enum Block: Hashable {
        case paragraph(String)
        case heading(String)
        case item(String, marker: String)
        case code(String)
    }

    private let blocks: [Block]

    init(_ markdown: String) {
        blocks = Self.parse(markdown)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .paragraph(let text): Text(Self.inline(text)).lineSpacing(4)
                case .heading(let text): Text(Self.inline(text)).font(.app(16, .semibold))
                case .item(let text, let marker):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(marker).foregroundStyle(Palette.muted)
                        Text(Self.inline(text)).lineSpacing(4)
                    }
                case .code(let text):
                    ScrollView(.horizontal, showsIndicators: false) { Text(text).font(.code(13)).padding(12) }
                        .background(Palette.raised, in: .rect(cornerRadius: 10))
                }
            }
        }
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private static func inline(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }

    private static func parse(_ markdown: String) -> [Block] {
        var blocks: [Block] = []
        var paragraph: [String] = []
        var code: [String]?
        func flush() {
            if !paragraph.isEmpty { blocks.append(.paragraph(paragraph.joined(separator: "\n"))) }
            paragraph = []
        }
        for line in markdown.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if let lines = code {
                    blocks.append(.code(lines.joined(separator: "\n")))
                    code = nil
                } else {
                    flush()
                    code = []
                }
            } else if code != nil {
                code?.append(line)
            } else if trimmed.isEmpty {
                flush()
            } else if let m = trimmed.wholeMatch(of: /#{1,6}\s+(.*)/) {
                flush()
                blocks.append(.heading(String(m.1)))
            } else if let m = trimmed.wholeMatch(of: /([-*+]|\d+[.)])\s+(.*)/) {
                flush()
                let marker = String(m.1)
                blocks.append(.item(String(m.2), marker: marker.first?.isNumber == true ? marker : "•"))
            } else {
                paragraph.append(trimmed)
            }
        }
        if let code { blocks.append(.code(code.joined(separator: "\n"))) }
        flush()
        return blocks
    }
}
