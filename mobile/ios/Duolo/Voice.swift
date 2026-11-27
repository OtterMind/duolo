import AVFoundation
import DuoloKit
import Observation
import SwiftUI

/// 语音识别：录麦克风，16 kHz PCM 发给配对的 Duolo 服务器识别（和电脑端同一个接口），带上任务的热词和对话。
@MainActor @Observable
final class VoiceInput {
    enum Phase: Equatable { case idle, starting, listening }

    private(set) var phase = Phase.idle
    private(set) var text = ""
    private(set) var level: CGFloat = 0
    private(set) var startedAt = Date.now

    private var engine: AVAudioEngine?
    private var stream: VoiceStream?
    private var final: String?
    /// 识别完按这些名字改正拆开说的词。
    private var hints = VoiceHints()
    private var finished: CheckedContinuation<Void, Never>?

    /// 说完后等最终结果最长多久。
    private static let finalTimeout: Duration = .seconds(20)

    func start(server: URL, token: String, hints: VoiceHints) async throws {
        guard phase == .idle else { return }
        phase = .starting
        text = ""
        level = 0
        final = nil
        self.hints = hints
        do {
            guard await AVAudioApplication.requestRecordPermission() else { throw AppError("voicePermission") }
            let stream = try await VoiceStream.open(server: server, token: token, hints: hints)
            // 等授权弹窗或连服务器的时候已经松手了。
            guard phase == .starting else {
                stream.cancel()
                return
            }
            self.stream = stream
            try listen(stream)
            phase = .listening
            startedAt = .now
            Task { [weak self] in
                for await event in stream.events { self?.receive(event) }
                self?.complete()
            }
        } catch {
            reset()
            throw error
        }
    }

    private func receive(_ event: VoiceStream.Event) {
        switch event {
        case .partial(let said): text = said
        case .final(let said):
            text = said
            final = said
            complete()
        case .failed: complete()
        }
    }

    private func listen(_ stream: VoiceStream) throws {
        let session = AVAudioSession.sharedInstance()
        // 戴着蓝牙耳机时用耳机的麦克风。
        let bluetooth: AVAudioSession.CategoryOptions = if #available(iOS 26, *) { .allowBluetoothHFP } else { .allowBluetooth }
        try session.setCategory(.record, mode: .measurement, options: [.duckOthers, bluetooth])
        try session.setActive(true, options: .notifyOthersOnDeactivation)

        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0,
            let target = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16000, channels: 1, interleaved: true),
            let converter = AVAudioConverter(from: format, to: target)
        else { throw AppError("voicePermission") }
        Self.tap(input, format: format, converter: converter, target: target, stream: stream) { [weak self] value in
            Task { @MainActor in self?.level = value }
        }
        engine.prepare()
        try engine.start()
        self.engine = engine
    }

    /// 在音频线程上被调用，不能带主线程隔离。
    private nonisolated static func tap(
        _ input: AVAudioInputNode, format: AVAudioFormat, converter: AVAudioConverter, target: AVAudioFormat, stream: VoiceStream,
        level: @escaping @Sendable (CGFloat) -> Void
    ) {
        nonisolated(unsafe) let converter = converter
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, _ in
            if let samples = buffer.floatChannelData?[0], buffer.frameLength > 0 {
                var sum: Float = 0
                for i in 0..<Int(buffer.frameLength) { sum += samples[i] * samples[i] }
                level(CGFloat(min(1, sqrt(sum / Float(buffer.frameLength)) * 12)))
            }
            let capacity = AVAudioFrameCount(Double(buffer.frameLength) * target.sampleRate / format.sampleRate) + 32
            guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return }
            nonisolated(unsafe) var fed = false
            var error: NSError?
            converter.convert(to: out, error: &error) { _, status in
                if fed {
                    status.pointee = .noDataNow
                    return nil
                }
                fed = true
                status.pointee = .haveData
                return buffer
            }
            guard error == nil, out.frameLength > 0, let pcm = out.int16ChannelData?[0] else { return }
            stream.send(Data(bytes: pcm, count: Int(out.frameLength) * MemoryLayout<Int16>.size))
        }
    }

    /// 停止录音，等服务器的最终结果。
    func stop() async -> String {
        guard phase == .listening, let stream else {
            reset()
            return ""
        }
        engine?.stop()
        engine?.inputNode.removeTap(onBus: 0)
        stream.stop()
        if final == nil {
            await withCheckedContinuation { c in
                finished = c
                Task {
                    try? await Task.sleep(for: Self.finalTimeout)
                    complete()
                }
            }
        }
        let said = hints.fixNames((final ?? text).trimmingCharacters(in: .whitespacesAndNewlines))
        reset()
        return said
    }

    func cancel() {
        complete()
        reset()
    }

    private func complete() {
        finished?.resume()
        finished = nil
    }

    private func reset() {
        engine?.stop()
        engine?.inputNode.removeTap(onBus: 0)
        stream?.cancel()
        engine = nil
        stream = nil
        phase = .idle
        level = 0
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}

/// 一次语音输入：从按下到说完。
@MainActor @Observable
final class VoiceSession {
    enum Target { case send, edit, cancel }

    let input = VoiceInput()
    private(set) var visible = false
    /// 轻点开始的：一直听，用面板上的按钮结束。
    private(set) var handsFree = false
    var target = Target.send
    private var done: (String, Bool) -> Void = { _, _ in }

    func begin(_ model: AppModel, done: @escaping (_ text: String, _ edit: Bool) -> Void) {
        guard !visible else { return }
        self.done = done
        target = .send
        handsFree = false
        visible = true
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        guard let route = model.voiceRoute() else {
            visible = false
            model.problem = .unauthorized
            return
        }
        let hints = model.voiceHints()
        Task {
            do {
                try await input.start(server: route.server, token: route.token, hints: hints)
            } catch {
                visible = false
                model.problem = error as? AppError ?? AppError("voiceFailed")
            }
        }
    }

    func keepListening() {
        handsFree = true
        target = .send
    }

    func finish(_ target: Target, _ model: AppModel) {
        guard visible else { return }
        guard target != .cancel else {
            input.cancel()
            visible = false
            return
        }
        Task {
            let wasListening = input.phase == .listening
            let text = await input.stop()
            visible = false
            if !text.isEmpty {
                done(text, target == .edit)
            } else if wasListening {
                model.problem = AppError("nothingRecognized")
            }
        }
    }
}

/// 按住说话：松开发送，上滑先改，左滑取消。轻点一下则一直听，用面板上的按钮结束（读屏也用这个）。
struct HoldToTalk: View {
    @Environment(AppModel.self) private var model
    /// 列表右下角的大圆按钮。
    var floating = false
    /// 说完的话；`edit` 为真时先放进输入框。
    let done: (_ text: String, _ edit: Bool) -> Void

    @State private var pressedAt: Date?

    var body: some View {
        Icon("mic", size: 22)
            .foregroundStyle(floating ? Palette.onGreen : Palette.text)
            .frame(width: floating ? 56 : 44, height: floating ? 56 : 44)
            .background(floating ? Palette.green : .clear, in: .circle)
            .contentShape(Circle())
            .gesture(press)
            .accessibilityElement()
            .accessibilityLabel(model.t("mobile.voice.hold"))
            .accessibilityAddTraits(.isButton)
            .accessibilityAction {
                model.voice.begin(model, done: done)
                model.voice.keepListening()
            }
    }

    private var press: some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .global)
            .onChanged { drag in
                if pressedAt == nil {
                    pressedAt = .now
                    model.voice.begin(model, done: done)
                }
                let next: VoiceSession.Target = drag.translation.height < -70 ? .edit : drag.translation.width < -70 ? .cancel : .send
                if next != model.voice.target {
                    model.voice.target = next
                    UISelectionFeedbackGenerator().selectionChanged()
                }
            }
            .onEnded { _ in
                let held = pressedAt.map { Date.now.timeIntervalSince($0) } ?? 0
                pressedAt = nil
                if held < 0.35 && model.voice.target == .send {
                    model.voice.keepListening()
                } else {
                    model.voice.finish(model.voice.target, model)
                }
            }
    }
}

/// 说话时的面板，从底部升起；背后的内容不压暗，边看边答。
struct VoiceOverlay: View {
    @Environment(AppModel.self) private var model

    private var voice: VoiceInput { model.voice.input }
    private var target: VoiceSession.Target { model.voice.target }

    private func finish(_ target: VoiceSession.Target) { model.voice.finish(target, model) }

    var body: some View {
        if model.voice.visible {
            VStack {
                Spacer()
                VStack(alignment: .leading, spacing: 24) {
                    Text(voice.text.isEmpty ? model.t("mobile.voice.hint") : voice.text)
                        .font(.app(20)).foregroundStyle(voice.text.isEmpty || target == .cancel ? Palette.muted : Palette.text)
                        .strikethrough(target == .cancel && !voice.text.isEmpty)
                        .frame(maxWidth: .infinity, minHeight: 60, alignment: .topLeading)
                    if model.voice.handsFree { buttons } else { hints }
                }
                .padding(24)
                .background(Palette.raised, in: UnevenRoundedRectangle(topLeadingRadius: 24, topTrailingRadius: 24))
                .background(Palette.raised.ignoresSafeArea(edges: .bottom).padding(.top, 40))
            }
            .foregroundStyle(Palette.text)
            // 按住的时候手指还在下面的按钮上，面板不接触摸。
            .allowsHitTesting(model.voice.handsFree)
            .transition(.move(edge: .bottom))
        }
    }

    private var hints: some View {
        let t = model.t
        return HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 8) {
                hint("arrow-up", t(target == .edit ? "mobile.voice.releaseEdit" : "mobile.voice.slideEdit"), on: target == .edit, Palette.greenText)
                hint("arrow-left", t(target == .cancel ? "mobile.voice.releaseCancel" : "mobile.voice.slideCancel"), on: target == .cancel, Palette.bad)
            }
            Spacer(minLength: 8)
            clock
            Icon(target == .cancel ? "x" : target == .edit ? "pencil" : "mic", size: 24)
                .foregroundStyle(Palette.onGreen).frame(width: 56, height: 56)
                .background(target == .cancel ? Palette.bad : Palette.green, in: .circle)
                .padding(8).background(Palette.button.opacity(0.5 + voice.level / 2), in: .circle)
                .accessibilityLabel(t("mobile.voice.releaseSend"))
        }
    }

    private var buttons: some View {
        let t = model.t
        return HStack(spacing: 12) {
            IconButton(icon: "x", label: t("common.cancel")) { finish(.cancel) }
            Spacer()
            clock
            IconButton(icon: "pencil", label: t("mobile.voice.releaseEdit")) { finish(.edit) }
            Button { finish(.send) } label: {
                Icon("arrow-up", size: 24).foregroundStyle(Palette.onGreen).frame(width: 56, height: 56).background(Palette.green, in: .circle)
            }
            .accessibilityLabel(t("mobile.send"))
        }
    }

    @ViewBuilder private var clock: some View {
        if voice.phase == .listening {
            Text(timerInterval: voice.startedAt...voice.startedAt.addingTimeInterval(3600), countsDown: false)
                .font(.app(14, relativeTo: .subheadline)).monospacedDigit().foregroundStyle(Palette.muted)
        }
    }

    private func hint(_ icon: String, _ text: String, on: Bool, _ color: Color) -> some View {
        HStack(spacing: 6) {
            Icon(icon, size: 16)
            Text(text)
        }
        .font(.app(14, on ? .semibold : .regular, relativeTo: .subheadline)).foregroundStyle(on ? color : Palette.muted)
    }
}
