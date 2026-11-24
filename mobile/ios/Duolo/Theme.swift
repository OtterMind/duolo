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

extension Color {
    /// `#rrggbb`，读不出来时为空。
    init?(hex: String) {
        guard hex.hasPrefix("#"), hex.count == 7, let value = UInt32(hex.dropFirst(), radix: 16) else { return nil }
        self.init(red: Double(value >> 16 & 0xFF) / 255, green: Double(value >> 8 & 0xFF) / 255, blue: Double(value & 0xFF) / 255)
    }
}

/// 写代码的 Agent 的图标：插件自带的（mobile/scripts/gen-agent-icons.mjs 生成），颜色由电脑发来；没有图标的显示首字母。
struct AgentMark: View {
    let agent: AgentOption
    var size: CGFloat = 14

    var body: some View {
        Group {
            if UIImage(named: "agent-\(agent.kind)") != nil {
                Image("agent-\(agent.kind)").renderingMode(.template).resizable().scaledToFit()
                    .foregroundStyle(agent.color.flatMap(Color.init(hex:)) ?? Palette.muted)
            } else {
                Text(agent.kind.prefix(1).uppercased()).font(.system(size: size * 0.72, weight: .semibold)).foregroundStyle(Palette.muted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).background(Palette.button, in: .rect(cornerRadius: 3))
            }
        }
        .frame(width: size, height: size)
        .accessibilityLabel(agent.name)
    }
}

/// 项目里的哪个仓库。
struct RepoTag: View {
    let name: String

    var body: some View {
        Text(name).font(.app(12, relativeTo: .caption)).foregroundStyle(Palette.muted).lineLimit(1)
            .padding(.horizontal, 6).padding(.vertical, 1).background(Palette.raised, in: .rect(cornerRadius: 5))
            .fixedSize()
    }
}

/// 任务状态的颜色、图标和名字，列表和详情都用这一份。
/// 状态图标的底色：两个主题一样，上面的白色图标都看得清；和电脑侧边栏同一组。
enum StateFill {
    static let wait = Color(red: 0xB0 / 255, green: 0x7A / 255, blue: 0x00 / 255)
    static let bad = Color(red: 0xE0 / 255, green: 0x41 / 255, blue: 0x3F / 255)
    static let write = Color(red: 0x1F / 255, green: 0x74 / 255, blue: 0xE0 / 255)
    static let review = Color(red: 0x82 / 255, green: 0x50 / 255, blue: 0xDF / 255)
    /// 比微信绿深一档，白色对勾才看得清。
    static let pass = Color(red: 0x05 / 255, green: 0x95 / 255, blue: 0x4A / 255)
    static let gray = Color(red: 0x6E / 255, green: 0x77 / 255, blue: 0x81 / 255)
}

struct StatusStyle {
    /// 圆里的白色图标；审出问题时写问题数，未启动时没有。
    let icon: String?
    let count: Int?
    let fill: Color
    /// 写成文字时的颜色。
    let color: Color
    let label: String
    let active: Bool

    init(_ state: TaskRow.State, _ t: Localizer) {
        active = state.group == .active
        // 文案和电脑共用 state.*。
        var issues: Int?
        if case .issues(let n) = state { issues = n }
        count = issues
        label = t("state.\(state.code)", ["count": "\(issues ?? 0)"])
        switch state {
        case .needsYou: (icon, fill, color) = ("ask", StateFill.wait, Palette.wait)
        case .issues: (icon, fill, color) = (nil, StateFill.bad, Palette.bad)
        case .reviewFailed: (icon, fill, color) = ("alert", StateFill.bad, Palette.bad)
        case .crashed: (icon, fill, color) = ("x", StateFill.bad, Palette.bad)
        case .writing: (icon, fill, color) = ("pencil", StateFill.write, Palette.write)
        case .reviewing: (icon, fill, color) = ("eye", StateFill.review, Palette.review)
        case .passed: (icon, fill, color) = ("check", StateFill.pass, Palette.greenText)
        case .reviewed: (icon, fill, color) = ("eye", StateFill.gray, Palette.gray)
        case .written: (icon, fill, color) = ("check", StateFill.gray, Palette.gray)
        case .ended: (icon, fill, color) = ("dash", StateFill.gray, Palette.gray)
        case .notStarted: (icon, fill, color) = (nil, StateFill.gray, Palette.gray)
        }
    }
}

/// 状态图标：纯色圆底加白色图标（和电脑侧边栏一样）。进行中的圆小一圈，外面一段弧线在转；
/// 未启动只有一个虚线圈。读屏念状态名。
struct StatusBadge: View {
    let style: StatusStyle
    var size: CGFloat = 32
    @Environment(\.accessibilityReduceMotion) private var still
    @State private var turned = false

    var body: some View {
        let ring = size / 16
        ZStack {
            if style.icon == nil && style.count == nil {
                Circle().strokeBorder(Palette.dim, style: StrokeStyle(lineWidth: ring * 0.75, dash: [ring * 1.5, ring * 1.6]))
                    .padding(ring * 0.75)
            } else {
                if style.active {
                    Circle().stroke(style.fill.opacity(0.28), lineWidth: ring)
                    Circle().trim(from: 0, to: 0.25).stroke(style.fill, style: StrokeStyle(lineWidth: ring, lineCap: .round))
                        .rotationEffect(.degrees(turned ? 270 : -90))
                }
                Circle().fill(style.fill).padding(style.active ? size * 0.11 : 0)
                if let count = style.count {
                    Text(count > 9 ? "9+" : "\(count)").font(.app(size * (count > 9 ? 0.38 : 0.47), .semibold)).monospacedDigit()
                        .foregroundStyle(.white).dynamicTypeSize(.large)
                } else if let icon = style.icon {
                    Image(icon).renderingMode(.template).resizable().scaledToFit().foregroundStyle(.white)
                        .frame(width: size * (style.active ? 0.4 : 0.5), height: size * (style.active ? 0.4 : 0.5))
                }
            }
        }
        .padding(style.active ? ring / 2 : 0)
        .frame(width: size, height: size)
        .onAppear {
            guard style.active, !still else { return }
            withAnimation(.linear(duration: 1.6).repeatForever(autoreverses: false)) { turned = true }
        }
        .id("\(style.label)\(style.active)")
        .accessibilityElement()
        .accessibilityLabel(style.label)
    }
}
