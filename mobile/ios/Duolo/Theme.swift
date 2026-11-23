import DuoloKit
import SwiftUI
import UIKit

/// 颜色，浅色和深色成对，跟随系统。和 Android 的 Theme.kt 一致。
enum Palette {
    static let bg = pair(0xFFFFFF, 0x161616)
    static let raised = pair(0xF3F5F7, 0x222222)
    static let bubble = pair(0xEEF1F4, 0x262626)
    static let button = pair(0xE1E5EA, 0x2E2E2E)
    static let text = pair(0x1C2127, 0xE4E4E4)
    static let muted = pair(0x57606E, 0xA3A3A3)
    static let dim = pair(0x66707E, 0x8C8C8C)
    static let line = pair(0xE7EAEE, 0x2A2A2A)

    static let green = pair(0x07C160, 0x07C160)
    /// 绿色按钮上的字。
    static let onGreen = pair(0x04210F, 0x04210F)
    /// 写成文字的绿：浅色主题用深一档，不然看不清。
    static let greenText = pair(0x007A3A, 0x07C160)
    static let greenSoft = pair(0xE6F8EF, 0x14301F)

    static let wait = pair(0xA15C00, 0xFFC24B)
    static let bad = pair(0xCF2F4A, 0xFF7A85)
    static let write = pair(0x0B6BCB, 0x5CB8FF)
    static let review = pair(0x7A4FD6, 0xB69CFF)
    static let gray = pair(0x5F6B7A, 0x9AA4B2)

    private static func pair(_ light: UInt32, _ dark: UInt32) -> Color {
        Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? rgb(dark) : rgb(light) })
    }

    private static func rgb(_ hex: UInt32) -> UIColor {
        UIColor(
            red: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}

extension Font {
    /// Mona Sans，大小跟随系统的字号设置；没有的字（中日韩等）系统自动换成自己的字体。
    static func app(_ size: CGFloat, _ weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom("Mona Sans", size: size, relativeTo: style).weight(weight)
    }

    static func code(_ size: CGFloat) -> Font { .system(size: size, design: .monospaced) }
}

/// GitHub Octicons。
struct Icon: View {
    let name: String
    var size: CGFloat = 18

    init(_ name: String, size: CGFloat = 18) {
        self.name = name
        self.size = size
    }

    @ScaledMetric(relativeTo: .body) private var scale = 1.0

    var body: some View {
        Image(name).renderingMode(.template).resizable().scaledToFit().frame(width: size * scale, height: size * scale).accessibilityHidden(true)
    }
}

/// 图标按钮：点按区至少 44×44，读屏时念 `label`。
struct IconButton: View {
    let icon: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) { Icon(icon).frame(width: 44, height: 44).contentShape(Rectangle()) }
            .foregroundStyle(Palette.text)
            .accessibilityLabel(label)
    }
}

struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.app(16, .medium))
            .multilineTextAlignment(.center)
            .padding(.horizontal, 16).padding(.vertical, 12)
            .frame(maxWidth: .infinity, minHeight: 50)
            .foregroundStyle(Palette.onGreen)
            .background(Palette.green, in: .rect(cornerRadius: 25))
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.45)
    }
}

struct SecondaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.app(16, .medium))
            .multilineTextAlignment(.center)
            .padding(.horizontal, 16).padding(.vertical, 12)
            .frame(maxWidth: .infinity, minHeight: 50)
            .foregroundStyle(Palette.text)
            .background(Palette.button, in: .rect(cornerRadius: 25))
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.45)
    }
}

/// 任务状态的颜色、图标和名字，列表和详情都用这一份。
struct StatusStyle {
    let icon: String
    let color: Color
    let label: String
    let active: Bool

    init(_ state: TaskRow.State, _ t: Localizer) {
        active = state.group == .active
        switch state {
        case .needsYou: (icon, color, label) = ("question", Palette.wait, t("status.needs_input"))
        case .issues(let n): (icon, color, label) = ("issue-opened", Palette.bad, t("mobile.state.issues", ["count": "\(n)"]))
        case .reviewFailed: (icon, color, label) = ("issue-opened", Palette.bad, t("review.failed"))
        case .crashed: (icon, color, label) = ("x-circle", Palette.bad, t("status.failed"))
        case .writing: (icon, color, label) = ("pencil", Palette.write, t("mobile.state.writing"))
        case .reviewing: (icon, color, label) = ("eye", Palette.review, t("mobile.state.reviewing"))
        case .passed: (icon, color, label) = ("check-circle", Palette.greenText, t("mobile.state.passed"))
        case .reviewed: (icon, color, label) = ("check-circle", Palette.gray, t("mobile.state.reviewed"))
        case .written: (icon, color, label) = ("check-circle", Palette.gray, t("mobile.state.written"))
        case .ended: (icon, color, label) = ("skip", Palette.gray, t("status.exited"))
        case .notStarted: (icon, color, label) = ("circle", Palette.gray, t("status.not_started"))
        }
    }
}

/// 状态图标：同样的圆角底，只换颜色和图标；进行中的会轻轻闪。
struct StatusBadge: View {
    let style: StatusStyle
    var size: CGFloat = 36
    @Environment(\.accessibilityReduceMotion) private var still
    @State private var dimmed = false

    var body: some View {
        Icon(style.icon, size: size / 2)
            .foregroundStyle(style.color)
            .opacity(dimmed ? 0.45 : 1)
            .frame(width: size, height: size)
            .background(style.color.opacity(0.13), in: .rect(cornerRadius: size * 0.3))
            .onAppear {
                guard style.active, !still else { return }
                withAnimation(.easeInOut(duration: 0.9).repeatForever()) { dimmed = true }
            }
            .id("\(style.icon)\(style.active)")
    }
}
