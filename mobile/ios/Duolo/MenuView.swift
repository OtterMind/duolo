import DuoloKit
import SwiftUI

/// 左边滑出的菜单：切换仓库和电脑、设置。
struct MenuView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.accessibilityReduceMotion) private var still
    @Binding var isPresented: Bool
    @State private var showSettings = false
    @State private var showPair = false

    var body: some View {
        ZStack(alignment: .leading) {
            if isPresented {
                Color.black.opacity(0.45).ignoresSafeArea().onTapGesture { close() }.accessibilityHidden(true).transition(.opacity)
                panel.transition(still ? .opacity : .move(edge: .leading))
            }
        }
        .animation(.easeOut(duration: 0.2), value: isPresented)
        .sheet(isPresented: $showSettings) { SettingsView().environment(model) }
        .fullScreenCover(isPresented: $showPair) { PairView { showPair = false }.environment(model).overlay(alignment: .top) { ProblemBanner() } }
        .onChange(of: model.computers.count) { showPair = false }
        #if DEBUG
        .onAppear { if DebugLaunch.value("duoloScreen") == "settings" { showSettings = true } }
        #endif
    }

    private func close() { isPresented = false }

    private var panel: some View {
        let t = model.t
        return VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    heading(t("mobile.menu.projects"))
                    ForEach(model.overview?.projects ?? []) { project in
                        let rows = model.rows(in: project.id)
                        let waiting = rows.filter { $0.group == .attention }.count
                        let active = rows.filter { $0.group == .active }.count
                        item("file-directory", project.name, selected: project.id == model.currentProject?.id) {
                            if waiting > 0 {
                                note(t("sidebar.needsYou", ["count": "\(waiting)"]), Palette.wait)
                            } else if active > 0 {
                                note(t("mobile.menu.active", ["count": "\(active)"]), Palette.write)
                            }
                        } action: {
                            model.choose(project: project.id)
                            close()
                        }
                    }
                    heading(t("mobile.menu.computers"))
                    ForEach(model.computers) { computer in
                        let current = computer.id == model.current?.id
                        item("device-desktop", computer.name, selected: current && model.computers.count > 1) {
                            if current { note(linkLabel(t), model.computerOnline ? Palette.greenText : Palette.dim) }
                        } action: {
                            model.select(computer)
                        }
                    }
                    item("plus", t("mobile.menu.add"), selected: false) {} action: { showPair = true }
                }
            }
            item("gear", t("settings.title"), selected: false) {} action: { showSettings = true }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .frame(maxWidth: 318, maxHeight: .infinity, alignment: .leading)
        .background(Palette.bg, ignoresSafeAreaEdges: .vertical)
        .accessibilityAddTraits(.isModal)
        .accessibilityAction(.escape) { close() }
    }

    private func linkLabel(_ t: Localizer) -> String {
        switch model.link {
        case .connected(let online): t(online ? "mobile.online" : "mobile.offline")
        case .connecting, .waiting: t("mobile.connecting")
        case .unlinked: t("mobile.offline")
        }
    }

    private func heading(_ text: String) -> some View {
        Text(text).font(.app(12, .semibold, relativeTo: .caption)).foregroundStyle(Palette.dim)
            .padding(.horizontal, 12).padding(.top, 20).padding(.bottom, 6).accessibilityAddTraits(.isHeader)
    }

    private func note(_ text: String, _ color: Color) -> some View {
        Text(text).font(.app(13, .semibold, relativeTo: .footnote)).foregroundStyle(color).lineLimit(1)
    }

    private func item(
        _ icon: String, _ label: String, selected: Bool, @ViewBuilder trailing: () -> some View, action: @escaping () -> Void
    ) -> some View {
        let trailing = trailing()
        return Button(action: action) {
            HStack(spacing: 14) {
                Icon(icon).foregroundStyle(selected ? Palette.greenText : Palette.muted)
                Text(label).font(.app(16, .medium)).lineLimit(1)
                Spacer(minLength: 8)
                trailing
            }
            .padding(.horizontal, 12).frame(minHeight: 48)
            .background(selected ? Palette.greenSoft : .clear, in: .rect(cornerRadius: 12))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var confirmDisconnect: Computer?
    @State private var showLicenses = false

    var body: some View {
        @Bindable var model = model
        let t = model.t
        NavigationStack {
            Form {
                Section(t("settings.general")) {
                    Picker(t("settings.language"), selection: $model.language) {
                        Text(t("sidebar.languageAuto")).tag("auto")
                        ForEach(Localizer.locales, id: \.code) { Text($0.name).tag($0.code) }
                    }
                    Picker(t("mobile.settings.appearance"), selection: $model.appearance) {
                        Text(t("mobile.settings.system")).tag("auto")
                        Text(t("mobile.settings.light")).tag("light")
                        Text(t("mobile.settings.dark")).tag("dark")
                    }
                }
                if let computer = model.current {
                    Section(computer.name) {
                        Button(t("mobile.settings.disconnect"), role: .destructive) { confirmDisconnect = computer }.foregroundStyle(Palette.bad)
                    }
                }
                Section {
                    Button(t("mobile.settings.licenses")) { showLicenses = true }.foregroundStyle(Palette.text)
                } footer: {
                    Text(t("mobile.settings.version", ["version": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""]))
                }
            }
            .navigationTitle(t("settings.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { IconButton(icon: "x", label: t("common.collapse")) { dismiss() } }
            }
        }
        .environment(\.locale, t.locale)
        .preferredColorScheme(model.colorScheme)
        .alert(
            t("mobile.settings.disconnectTitle", ["computer": confirmDisconnect?.name ?? ""]),
            isPresented: Binding(get: { confirmDisconnect != nil }, set: { if !$0 { confirmDisconnect = nil } }), presenting: confirmDisconnect
        ) { computer in
            Button(t("mobile.settings.disconnectAction"), role: .destructive) {
                Task {
                    await model.disconnect(computer)
                    dismiss()
                }
            }
            Button(t("common.cancel"), role: .cancel) {}
        } message: { _ in
            Text(t("mobile.settings.disconnectBody"))
        }
        .sheet(isPresented: $showLicenses) {
            ScrollView { Text(Self.licenses).font(.code(12)).padding(20).frame(maxWidth: .infinity, alignment: .leading) }
        }
    }

    private static var licenses: String {
        [("LICENSES", "md"), ("OFL", "txt")].compactMap { Bundle.main.url(forResource: $0.0, withExtension: $0.1) }
            .compactMap { try? String(contentsOf: $0, encoding: .utf8) }.joined(separator: "\n\n")
    }
}
