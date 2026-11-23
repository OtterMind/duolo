import DuoloKit
import SwiftUI

/// 新任务：整页只写需求，其他按电脑上的默认值，要改再展开。
struct NewTaskView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    @State private var prompt: String
    @State private var expanded = false
    @State private var project: String?
    @State private var base: String?
    @State private var writer: String?
    @State private var reviewer: String?
    @State private var sending = false
    @FocusState private var focused: Bool

    init(initial: String = "") {
        _prompt = State(initialValue: initial)
    }

    private var overview: Overview? { model.overview }
    private var chosen: ProjectOverview? { overview?.projects.first { $0.id == project } ?? model.currentProject }
    private var writerKind: String { writer ?? overview?.writer ?? "" }
    private var reviewerKind: String { reviewer ?? overview?.reviewer ?? "" }
    private var text: String { prompt.trimmingCharacters(in: .whitespacesAndNewlines) }

    private func name(_ kind: String) -> String { overview?.agents.first { $0.kind == kind }?.name ?? kind.capitalized }

    var body: some View {
        let t = model.t
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                IconButton(icon: "x", label: t("common.cancel")) { dismiss() }
                Spacer()
                Button(t("mobile.send"), action: send)
                    .font(.app(16, .medium)).foregroundStyle(Palette.onGreen).padding(.horizontal, 20).frame(minHeight: 44)
                    .background(Palette.green, in: .capsule).opacity(text.isEmpty || sending ? 0.45 : 1)
                    .disabled(text.isEmpty || sending || chosen == nil)
            }
            .padding(.leading, 8).padding(.trailing, 16).padding(.top, 4)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    TextField(t("mobile.new.prompt"), text: $prompt, axis: .vertical)
                        .font(.app(20)).lineLimit(4...16).focused($focused)
                    HStack(spacing: 8) {
                        Button { withAnimation { expanded.toggle() } } label: {
                            HStack(spacing: 6) {
                                Text(t("mobile.new.summary", ["project": chosen?.name ?? "", "writer": name(writerKind), "reviewer": name(reviewerKind)]))
                                    .multilineTextAlignment(.leading)
                                Icon("chevron-down", size: 14).rotationEffect(.degrees(expanded ? 180 : 0))
                            }
                            .font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted).frame(minHeight: 44).contentShape(Rectangle())
                        }
                        Spacer(minLength: 0)
                        HoldToTalk { said, _ in prompt = prompt.isEmpty ? said : prompt + "\n" + said }.frame(width: 44, height: 44)
                    }
                    if expanded { options }
                    if !model.computerOnline {
                        Text(t("mobile.new.queued")).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted)
                    }
                }
                .padding(.horizontal, 24).padding(.top, 16)
            }
        }
        .background(Palette.bg)
        .foregroundStyle(Palette.text)
        .font(.app(16))
        .overlay { VoiceOverlay() }
        .overlay(alignment: .top) { ProblemBanner() }
        .onAppear { focused = prompt.isEmpty }
    }

    private var options: some View {
        let t = model.t
        let agents = overview?.agents ?? []
        return VStack(spacing: 0) {
            Palette.line.frame(height: 1).padding(.bottom, 8)
            picker(t("newTask.project"), chosen?.name ?? "", (overview?.projects ?? []).map { ($0.id, $0.name) }) {
                project = $0
                base = nil
            }
            picker(t("newTask.baseBranch"), base ?? chosen?.branch ?? "", (chosen?.branches ?? []).map { ($0, $0) }) { base = $0 }
            picker(t("newTask.writerTitle"), name(writerKind), agents.map { ($0.kind, $0.name) }) { writer = $0 }
            picker(t("newTask.reviewerTitle"), name(reviewerKind), agents.map { ($0.kind, $0.name) }) { reviewer = $0 }
        }
    }

    private func picker(_ label: String, _ value: String, _ items: [(String, String)], choose: @escaping (String) -> Void) -> some View {
        Menu {
            ForEach(items, id: \.0) { item in Button(item.1) { choose(item.0) } }
        } label: {
            HStack(spacing: 12) {
                Text(label).foregroundStyle(Palette.muted)
                Spacer(minLength: 8)
                Text(value).foregroundStyle(Palette.text).multilineTextAlignment(.trailing)
                Icon("chevron-down", size: 12).foregroundStyle(Palette.dim)
            }
            .frame(minHeight: 48).contentShape(Rectangle())
        }
        .disabled(items.isEmpty)
    }

    private func send() {
        guard let chosen else { return }
        sending = true
        Task {
            let ok = await model.submit(.createTask(project: chosen.id, prompt: text, base: base, writer: writer, reviewer: reviewer))
            sending = false
            if ok {
                model.choose(project: chosen.id)
                dismiss()
            }
        }
    }
}
