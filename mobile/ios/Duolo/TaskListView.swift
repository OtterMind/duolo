import DuoloKit
import SwiftUI

struct TaskListView: View {
    enum Filter: CaseIterable { case all, attention, active, done }

    struct NewTask: Identifiable {
        let id = UUID()
        var text = ""
    }

    @Environment(AppModel.self) private var model
    @State private var path: [Int64] = []
    @State private var filter = Filter.all
    @State private var searching = false
    @State private var query = ""
    @State private var showMenu = false
    /// 打开新任务页；`text` 是说完先改再发的话。
    @State private var newTask: NewTask?
    @FocusState private var searchFocused: Bool

    private var all: [TaskRow] { model.rows(in: model.currentProject?.id) }

    private func matches(_ row: TaskRow, _ filter: Filter) -> Bool {
        switch filter {
        case .all: true
        case .attention: row.group == .attention
        case .active: row.group == .active
        case .done: row.group == .done
        }
    }

    private var visible: [TaskRow] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return all.filter { row in
            matches(row, filter)
                && (q.isEmpty || model.title(row.task).localizedCaseInsensitiveContains(q) || row.detail.localizedCaseInsensitiveContains(q)
                    || row.task.branch.localizedCaseInsensitiveContains(q))
        }
    }

    var body: some View {
        NavigationStack(path: $path) {
            VStack(spacing: 0) {
                if searching { searchBar } else { header }
                filters
                LinkNotice()
                list
            }
            .background(Palette.bg)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: Int64.self) { TaskView(id: $0) }
            .overlay(alignment: .bottomTrailing) {
                if let project = model.currentProject {
                    HoldToTalk(floating: true) { text, edit in
                        if edit {
                            newTask = NewTask(text: text)
                        } else {
                            Task { await model.submit(.createTask(project: project.id, prompt: text, base: nil, writer: nil, reviewer: nil)) }
                        }
                    }
                    .padding(.trailing, 16).padding(.bottom, 8)
                }
            }
        }
        .fullScreenCover(item: $newTask) { NewTaskView(initial: $0.text).environment(model) }
        .overlay { MenuView(isPresented: $showMenu) }
        #if DEBUG
        .task(id: model.rows.isEmpty) {
            guard !model.rows.isEmpty else { return }
            if let id = DebugLaunch.value("duoloOpen").flatMap(Int64.init) { path = [id] }
            switch DebugLaunch.value("duoloScreen") {
            case "new": newTask = NewTask(text: DebugLaunch.value("duoloDraft") ?? "")
            case "menu", "settings", "pair": showMenu = true
            case "voice":
                model.voice.begin(model) { _, _ in }
                model.voice.keepListening()
            default: break
            }
        }
        #endif
    }

    private var header: some View {
        let t = model.t
        return HStack(spacing: 0) {
            Button { showMenu = true } label: {
                Icon("three-bars").frame(width: 44, height: 44)
                    .overlay(alignment: .topTrailing) {
                        if model.othersNeedYou { Circle().fill(Palette.wait).frame(width: 8, height: 8).padding(9) }
                    }
                    .contentShape(Rectangle())
            }
            .accessibilityLabel(t("mobile.menu.title"))
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Icon("repo", size: 16).foregroundStyle(Palette.muted)
                    Text(model.currentProject?.name ?? "Duolo").font(.app(17, .semibold)).lineLimit(1)
                }
                HStack(spacing: 6) {
                    Circle().fill(model.computerOnline ? Palette.green : Palette.dim).frame(width: 7, height: 7)
                    Text(model.current?.name ?? "").font(.app(12, relativeTo: .caption)).foregroundStyle(Palette.muted).lineLimit(1)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(model.current?.name ?? ""), \(t(model.computerOnline ? "mobile.online" : "mobile.offline"))")
            }
            .padding(.leading, 4)
            Spacer(minLength: 8)
            IconButton(icon: "search", label: t("sidebar.search")) {
                searching = true
                searchFocused = true
            }
            Button { newTask = NewTask() } label: {
                Icon("plus").foregroundStyle(Palette.greenText).frame(width: 36, height: 36)
                    .background(Palette.greenSoft, in: .circle).frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .accessibilityLabel(t("sidebar.newTask"))
            .disabled(model.currentProject == nil)
        }
        .padding(.horizontal, 6).padding(.top, 4)
    }

    private var searchBar: some View {
        let t = model.t
        return HStack(spacing: 8) {
            HStack(spacing: 8) {
                Icon("search", size: 16).foregroundStyle(Palette.muted)
                TextField(t("sidebar.search"), text: $query).focused($searchFocused).submitLabel(.search).autocorrectionDisabled()
            }
            .padding(.horizontal, 14).frame(minHeight: 44).background(Palette.raised, in: .rect(cornerRadius: 22))
            Button(t("common.cancel")) {
                query = ""
                searching = false
            }
            .frame(minHeight: 44)
        }
        .padding(.horizontal, 16).padding(.top, 4)
    }

    private var filters: some View {
        let t = model.t
        return ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Filter.allCases, id: \.self) { f in
                    let count = all.filter { matches($0, f) }.count
                    Button { filter = f } label: {
                        HStack(spacing: 6) {
                            Text(label(f, t))
                            Text("\(count)").monospacedDigit()
                        }
                        .font(.app(14, .medium, relativeTo: .subheadline))
                        .padding(.horizontal, 12).frame(minHeight: 32)
                        .foregroundStyle(filter == f ? Palette.greenText : Palette.muted)
                        .background(filter == f ? Palette.greenSoft : Palette.raised, in: .capsule)
                        .frame(minHeight: 44).contentShape(Rectangle())
                    }
                    .accessibilityAddTraits(filter == f ? .isSelected : [])
                }
            }
            .padding(.horizontal, 16)
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(.vertical, 4)
    }

    private func label(_ f: Filter, _ t: Localizer) -> String {
        switch f {
        case .all: t("mobile.filter.all")
        case .attention: t("mobile.filter.attention")
        case .active: t("mobile.filter.active")
        case .done: t("mobile.filter.done")
        }
    }

    @ViewBuilder private var list: some View {
        let t = model.t
        if model.overview == nil {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(newTasksWaiting) { PendingRow(item: $0) }
                    ForEach(visible) { row in
                        NavigationLink(value: row.id) { TaskRowView(row: row) }.buttonStyle(.plain)
                    }
                }
                .padding(.bottom, 96)
                if visible.isEmpty && newTasksWaiting.isEmpty {
                    VStack(spacing: 6) {
                        Text(all.isEmpty ? t("sidebar.noTasks") : t("mobile.empty.filtered")).font(.app(16, .medium))
                        if all.isEmpty {
                            Text(t("mobile.empty.hint")).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted)
                                .multilineTextAlignment(.center)
                        }
                    }
                    .padding(40)
                }
            }
            .refreshable { await model.refresh() }
        }
    }

    /// 电脑还没收到的新任务，也要看得到。
    private var newTasksWaiting: [OutboxItem] {
        guard query.isEmpty, filter == .all else { return [] }
        return model.outbox.filter {
            if case .createTask(let project, _, _, _, _) = $0.command { return project == model.currentProject?.id }
            return false
        }
    }
}

struct TaskRowView: View {
    @Environment(AppModel.self) private var model
    let row: TaskRow

    var body: some View {
        let style = StatusStyle(row.state, model.t)
        HStack(spacing: 14) {
            StatusBadge(style: style)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(model.title(row.task)).font(.app(16, .semibold)).lineLimit(1)
                    Spacer(minLength: 0)
                    Text(style.label).font(.app(13, .semibold, relativeTo: .footnote)).foregroundStyle(style.color).lineLimit(1).layoutPriority(1)
                }
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(row.detail).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted).lineLimit(1)
                    Spacer(minLength: 0)
                    TimelineView(.periodic(from: .now, by: 30)) { _ in
                        Text(row.since.map(model.lasting) ?? model.ago(row.task.updatedAt))
                    }
                    .font(.app(12, relativeTo: .caption)).foregroundStyle(Palette.dim).lineLimit(1).layoutPriority(1)
                }
            }
            .padding(.vertical, 13)
            .overlay(alignment: .bottom) { Palette.line.frame(height: 1) }
        }
        .padding(.horizontal, 16)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// 还没送到电脑的新任务。
struct PendingRow: View {
    @Environment(AppModel.self) private var model
    let item: OutboxItem

    var body: some View {
        if case .createTask(_, let prompt, _, _, _) = item.command {
            let t = model.t
            let failed = item.state == .failed
            HStack(spacing: 14) {
                Icon("clock").foregroundStyle(failed ? Palette.bad : Palette.gray).frame(width: 36, height: 36)
                    .background((failed ? Palette.bad : Palette.gray).opacity(0.13), in: .rect(cornerRadius: 11))
                VStack(alignment: .leading, spacing: 3) {
                    Text(prompt).font(.app(16, .semibold)).lineLimit(1)
                    Text(failed ? t.error(item.error ?? "internal") : t("mobile.pending"))
                        .font(.app(14, relativeTo: .subheadline)).foregroundStyle(failed ? Palette.bad : Palette.muted)
                }
                Spacer(minLength: 0)
                if failed {
                    IconButton(icon: "x", label: t("common.cancel")) { model.discard(item) }
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 8)
            .contentShape(Rectangle())
            .onTapGesture { if failed { model.retry(item) } }
        }
    }
}

/// 连接有问题时一行说明；正常时什么都不显示。
struct LinkNotice: View {
    @Environment(AppModel.self) private var model
    /// 正有提示要回答：回答不排队，离线时说清楚。
    var answering = false
    /// 刚断开不马上显示，免得一闪一闪。
    @State private var settled = false

    private var text: String? {
        let t = model.t
        switch model.link {
        case .connected(true), .connecting: return nil
        case .connected(false): return t(answering ? "error.computerOffline" : "mobile.banner.offline")
        case .waiting: return settled ? t("mobile.banner.network") : nil
        case .unlinked: return t("mobile.banner.unlinked")
        }
    }

    var body: some View {
        Group {
            if let text {
                Text(text).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).padding(.vertical, 6)
            }
        }
        .task(id: model.link) {
            settled = false
            try? await Task.sleep(for: .seconds(3))
            if !Task.isCancelled { settled = true }
        }
    }
}
