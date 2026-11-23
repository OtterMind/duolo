import AVFoundation
import DuoloKit
import Observation
import Speech
import SwiftUI

/// 语音识别：系统自带的 Speech。
@MainActor @Observable
final class VoiceInput {
    enum Phase: Equatable { case idle, starting, listening }
    enum Failure: Error { case permission, unavailable }

    private(set) var phase = Phase.idle
    private(set) var text = ""
    private(set) var level: CGFloat = 0
    private(set) var startedAt = Date.now

    private var engine: AVAudioEngine?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var finished: CheckedContinuation<Void, Never>?

    func start(locale: Locale) async throws {
        guard phase == .idle else { return }
        phase = .starting
        text = ""
        level = 0
        do {
            guard await Self.allowed() else { throw Failure.permission }
            // 等授权弹窗的时候已经松手了。
            guard phase == .starting else { return }
            try listen(locale: locale)
            phase = .listening
            startedAt = .now
        } catch {
            reset()
            throw error
        }
    }

    /// 系统在后台线程回调授权结果：回调不能带主线程隔离，不然 Swift 6 运行时检查会直接终止应用。
    private nonisolated static func allowed() async -> Bool {
        let speech = await withCheckedContinuation { (c: CheckedContinuation<SFSpeechRecognizerAuthorizationStatus, Never>) in
            SFSpeechRecognizer.requestAuthorization { c.resume(returning: $0) }
        }
        guard speech == .authorized else { return false }
        return await AVAudioApplication.requestRecordPermission()
    }

    private func listen(locale: Locale) throws {
        guard let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer(), recognizer.isAvailable else { throw Failure.unavailable }
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement, options: .duckOthers)
        try session.setActive(true, options: .notifyOthersOnDeactivation)

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw Failure.unavailable }
        Self.tap(input, format: format, request: request) { [weak self] value in
            Task { @MainActor in self?.level = value }
        }
        engine.prepare()
        try engine.start()
        self.engine = engine
        self.request = request
        task = Self.recognize(recognizer, request) { [weak self] text, done in
            Task { @MainActor in
                guard let self else { return }
                if let text { self.text = text }
                if done { self.complete() }
            }
        }
    }

    /// 这两个在音频线程和识别线程上被调用，不能带主线程隔离。
    private nonisolated static func tap(
        _ input: AVAudioInputNode, format: AVAudioFormat, request: SFSpeechAudioBufferRecognitionRequest, level: @escaping @Sendable (CGFloat) -> Void
    ) {
        nonisolated(unsafe) let request = request
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
            request.append(buffer)
            guard let samples = buffer.floatChannelData?[0], buffer.frameLength > 0 else { return }
            var sum: Float = 0
            for i in 0..<Int(buffer.frameLength) { sum += samples[i] * samples[i] }
            level(CGFloat(min(1, sqrt(sum / Float(buffer.frameLength)) * 12)))
        }
    }

    private nonisolated static func recognize(
        _ recognizer: SFSpeechRecognizer, _ request: SFSpeechRecognitionRequest, update: @escaping @Sendable (String?, Bool) -> Void
    ) -> SFSpeechRecognitionTask {
        recognizer.recognitionTask(with: request) { result, error in
            update(result?.bestTranscription.formattedString, error != nil || result?.isFinal == true)
        }
    }

    /// 停止录音，等最后的识别结果（最多 1.5 秒）。
    func stop() async -> String {
        guard phase == .listening else {
            reset()
            return ""
        }
        engine?.stop()
        engine?.inputNode.removeTap(onBus: 0)
        request?.endAudio()
        await withCheckedContinuation { c in
            finished = c
            Task {
                try? await Task.sleep(for: .milliseconds(1500))
                complete()
            }
        }
        let said = text.trimmingCharacters(in: .whitespacesAndNewlines)
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
        task?.cancel()
        engine = nil
        request = nil
        task = nil
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
        Task {
            do {
                try await input.start(locale: model.t.locale)
            } catch {
                visible = false
                model.problem = AppError(error as? VoiceInput.Failure == .permission ? "voicePermission" : "voiceUnavailable")
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
