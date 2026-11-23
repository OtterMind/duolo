import DuoloKit
import SwiftUI

@main
struct DuoloApp: App {
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .environment(\.locale, model.t.locale)
                .preferredColorScheme(model.colorScheme)
                .tint(Palette.greenText)
                .onOpenURL { model.handle(url: $0) }
                .onChange(of: phase) { _, next in
                    switch next {
                    case .active: model.becameActive()
                    case .background: model.wentToBackground()
                    default: break
                    }
                }
        }
    }
}

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        Group {
            if model.computers.isEmpty {
                PairView()
            } else {
                TaskListView()
            }
        }
        .foregroundStyle(Palette.text)
        .font(.app(16))
        .overlay { VoiceOverlay() }
        .overlay(alignment: .top) { ProblemBanner() }
        .sheet(item: $model.pendingPairing) { link in
            PairConfirmView(link: link).presentationDetents([.medium]).environment(model)
        }
        #if DEBUG
        .task { await DebugLaunch.run(model) }
        #endif
    }
}

extension PairingLink: @retroactive Identifiable {
    public var id: String { code }
}

/// 出错时在顶部显示几秒。
struct ProblemBanner: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let problem = model.problem {
            Text(model.message(problem))
                .font(.app(14))
                .foregroundStyle(Palette.text)
                .padding(.horizontal, 16).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Palette.raised, in: .rect(cornerRadius: 16))
                .overlay(RoundedRectangle(cornerRadius: 16).stroke(Palette.line))
                .padding(.horizontal, 16)
                .transition(.move(edge: .top).combined(with: .opacity))
                .onTapGesture { model.problem = nil }
                .accessibilityAddTraits(.isStaticText)
                .task(id: problem) {
                    AccessibilityNotification.Announcement(model.message(problem)).post()
                    try? await Task.sleep(for: .seconds(5))
                    if !Task.isCancelled { model.problem = nil }
                }
        }
    }
}

#if DEBUG
/// 自动化检查用：`-duoloPair <链接>` 直接配对，`-duoloOpen <任务编号>` 打开任务，`-duoloScreen new|menu|settings|pair|voice`。
enum DebugLaunch {
    static func value(_ name: String) -> String? { UserDefaults.standard.string(forKey: name) }

    @MainActor static func run(_ model: AppModel) async {
        if let text = value("duoloPair"), model.computers.isEmpty, let link = PairingLink(text) {
            do { try await model.pair(link) } catch { model.problem = error as? AppError }
        }
    }
}
#endif
