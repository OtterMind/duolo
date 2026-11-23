import AVFoundation
import DuoloKit
import SwiftUI
import UIKit

/// 第一次打开：连接电脑。
struct PairView: View {
    @Environment(AppModel.self) private var model
    @State private var scanning = false
    /// 在菜单里再加一台时，可以退出。
    var onCancel: (() -> Void)?

    var body: some View {
        let t = model.t
        VStack(alignment: .leading, spacing: 0) {
            if let onCancel {
                IconButton(icon: "x", label: t("common.cancel"), action: onCancel).padding(.leading, -12)
            }
            Spacer()
            Logo().padding(.bottom, 32)
            Text(t("mobile.pair.title")).font(.app(28, .semibold, relativeTo: .title)).accessibilityAddTraits(.isHeader)
            Text(t("mobile.pair.body")).foregroundStyle(Palette.muted).lineSpacing(4).padding(.top, 12)
            Spacer()
            VStack(spacing: 8) {
                Button(t("mobile.pair.scan")) { scanning = true }.buttonStyle(PrimaryButtonStyle())
                Button(t("mobile.pair.paste"), action: paste).buttonStyle(SecondaryButtonStyle())
                Text(t("mobile.pair.privacy")).font(.app(13, relativeTo: .footnote)).foregroundStyle(Palette.dim)
                    .multilineTextAlignment(.center).frame(maxWidth: .infinity).padding(.top, 8)
            }
        }
        .padding(.horizontal, 24).padding(.bottom, 16)
        .background(Palette.bg)
        .fullScreenCover(isPresented: $scanning) {
            ScannerView { text in
                scanning = false
                found(text)
            }
            .environment(model)
        }
    }

    private func paste() { found(UIPasteboard.general.string ?? "") }

    private func found(_ text: String) {
        if let link = PairingLink(text) { model.pendingPairing = link } else { model.problem = .badPairingLink }
    }
}

/// 标志：实心圆叠一个圆环，代表一写一审。和应用图标一致。
struct Logo: View {
    var body: some View {
        HStack(spacing: -26) {
            Circle().frame(width: 38, height: 38).zIndex(1)
            Circle().strokeBorder(lineWidth: 4.4).frame(width: 38, height: 38)
        }
        .foregroundStyle(Palette.text)
        .accessibilityHidden(true)
    }
}

/// 确认要连到哪个服务器：链接可能来自别的应用，不能不问就连。
struct PairConfirmView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let link: PairingLink
    @State private var working = false
    @State private var failure: AppError?

    var body: some View {
        let t = model.t
        VStack(alignment: .leading, spacing: 16) {
            Text(t("mobile.pair.confirm", ["server": link.serverName])).font(.app(20, .semibold, relativeTo: .title3))
            Text(t("mobile.pair.privacy")).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.muted)
            if let failure {
                Text(t.error(failure.code)).font(.app(14, relativeTo: .subheadline)).foregroundStyle(Palette.bad)
            }
            Spacer(minLength: 0)
            Button(working ? t("mobile.connecting") : t("mobile.pair.connect"), action: connect).buttonStyle(PrimaryButtonStyle()).disabled(working)
            Button(t("common.cancel")) { dismiss() }.buttonStyle(SecondaryButtonStyle()).disabled(working)
        }
        .padding(24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .foregroundStyle(Palette.text)
        .background(Palette.bg)
        .interactiveDismissDisabled(working)
    }

    private func connect() {
        working = true
        failure = nil
        Task {
            do {
                try await model.pair(link)
                dismiss()
            } catch {
                failure = error as? AppError ?? .server
            }
            working = false
        }
    }
}

/// 扫二维码。
struct ScannerView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var denied = false
    let found: @MainActor @Sendable (String) -> Void

    var body: some View {
        let t = model.t
        ZStack(alignment: .topLeading) {
            if denied {
                VStack(spacing: 16) {
                    Text(t("mobile.pair.camera")).multilineTextAlignment(.center)
                    Button(t("mobile.openSettings")) {
                        if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                }
                .padding(32).frame(maxWidth: .infinity, maxHeight: .infinity).background(Palette.bg)
            } else {
                CameraPreview(found: found).ignoresSafeArea().accessibilityLabel(t("mobile.pair.scan"))
            }
            IconButton(icon: "x", label: t("common.cancel")) { dismiss() }
                .background(.ultraThinMaterial, in: .circle).padding(12)
        }
        .task { denied = await !AVCaptureDevice.requestAccess(for: .video) }
    }
}

private struct CameraPreview: UIViewRepresentable {
    let found: @MainActor @Sendable (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(found: found) }

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.previewLayer.videoGravity = .resizeAspectFill
        view.previewLayer.session = context.coordinator.session
        context.coordinator.start()
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {}

    static func dismantleUIView(_ view: PreviewView, coordinator: Coordinator) { coordinator.stop() }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var previewLayer: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    /// 会话的开关在自己的队列上做，不卡界面。
    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate, @unchecked Sendable {
        let session = AVCaptureSession()
        private let queue = DispatchQueue(label: "duolo.scanner")
        private let found: @MainActor @Sendable (String) -> Void
        private var done = false

        init(found: @escaping @MainActor @Sendable (String) -> Void) { self.found = found }

        func start() {
            queue.async { [self] in
                guard let camera = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: camera),
                    session.canAddInput(input)
                else { return }
                session.addInput(input)
                let output = AVCaptureMetadataOutput()
                guard session.canAddOutput(output) else { return }
                session.addOutput(output)
                output.setMetadataObjectsDelegate(self, queue: queue)
                output.metadataObjectTypes = [.qr]
                session.startRunning()
            }
        }

        func stop() { queue.async { [self] in session.stopRunning() } }

        func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
            guard !done, let code = objects.compactMap({ ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }).first else { return }
            done = true
            Task { @MainActor [found] in found(code) }
        }
    }
}
