import Foundation

/// 一次语音识别：16 kHz、16 位、单声道 PCM 通过 WebSocket 发给服务器的 `/v1/voice`，收回识别中和最终的文字。
/// 协议见 server/src/voice.ts，和电脑端（crates/duolo-voice/src/cloud.rs）一样。
public final class VoiceStream: Sendable {
    public enum Event: Sendable, Equatable {
        /// 整段到目前为止的文字。
        case partial(String)
        /// 最终结果，之后不会再有。
        case final(String)
        case failed(AppError)
    }

    /// 连服务器、等它说"可以"最长多久。
    static let connectTimeout: Duration = .seconds(3)
    /// 服务器用这个关闭码表示登录凭据无效。
    static let closeUnauthorized = 4401

    private let task: URLSessionWebSocketTask
    public let events: AsyncStream<Event>

    private init(task: URLSessionWebSocketTask) {
        self.task = task
        let (events, continuation) = AsyncStream<Event>.makeStream()
        self.events = events
        Task { await Self.listen(task, continuation) }
    }

    /// 连上服务器并等它说"可以"。服务器不能用时抛出 `voiceUnavailable`、`voiceQuotaExceeded`、`voiceOffline` 或 `unauthorized`。
    public static func open(server: URL, token: String, hints: VoiceHints, session: URLSession = .shared) async throws -> VoiceStream {
        guard let url = voiceURL(server) else { throw AppError("voiceOffline") }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "authorization")
        let task = session.webSocketTask(with: request)
        task.resume()
        let timer = Task {
            try await Task.sleep(for: connectTimeout)
            task.cancel(with: .goingAway, reason: nil)
        }
        defer { timer.cancel() }
        do {
            let start = try JSONSerialization.data(withJSONObject: ["type": "start", "hotwords": hints.hotwords, "context": hints.context])
            try await task.send(.string(String(decoding: start, as: UTF8.self)))
            while true {
                guard case .string(let text) = try await task.receive() else { continue }
                switch Message(text) {
                case .ready: return VoiceStream(task: task)
                case .error(let code): throw AppError(code)
                default: continue
                }
            }
        } catch let error as AppError {
            task.cancel(with: .normalClosure, reason: nil)
            throw error
        } catch {
            let unauthorized = task.closeCode.rawValue == closeUnauthorized
            task.cancel(with: .goingAway, reason: nil)
            throw unauthorized ? AppError.unauthorized : AppError("voiceOffline")
        }
    }

    /// 一块声音（建议 100 毫秒左右）。
    public func send(_ pcm: Data) {
        task.send(.data(pcm)) { _ in }
    }

    /// 说完了：之后会收到 `.final`。
    public func stop() {
        task.send(.string(#"{"type":"stop"}"#)) { _ in }
    }

    public func cancel() {
        task.cancel(with: .normalClosure, reason: nil)
    }

    private static func listen(_ task: URLSessionWebSocketTask, _ continuation: AsyncStream<Event>.Continuation) async {
        defer { continuation.finish() }
        while true {
            guard let message = try? await task.receive() else {
                continuation.yield(.failed(AppError("voiceFailed")))
                return
            }
            guard case .string(let text) = message else { continue }
            switch Message(text) {
            case .partial(let said): continuation.yield(.partial(said))
            case .final(let said):
                continuation.yield(.final(said))
                return
            case .error(let code):
                continuation.yield(.failed(AppError(code)))
                return
            default: continue
            }
        }
    }

    /// `https://host/path` → `wss://host/path/v1/voice`（http 对应 ws）。
    static func voiceURL(_ server: URL) -> URL? {
        guard var parts = URLComponents(url: server, resolvingAgainstBaseURL: false) else { return nil }
        switch parts.scheme?.lowercased() {
        case "https": parts.scheme = "wss"
        case "http": parts.scheme = "ws"
        default: return nil
        }
        while parts.path.hasSuffix("/") { parts.path.removeLast() }
        parts.path += "/v1/voice"
        return parts.url
    }

    enum Message: Equatable {
        case ready, partial(String), final(String), error(String), other

        init(_ text: String) {
            struct Raw: Decodable { let type: String; let text: String?; let error: String? }
            guard let raw = try? JSONDecoder().decode(Raw.self, from: Data(text.utf8)) else {
                self = .other
                return
            }
            switch raw.type {
            case "ready": self = .ready
            case "partial": self = .partial(raw.text ?? "")
            case "final": self = .final(raw.text ?? "")
            case "error": self = .error(raw.error ?? "voiceFailed")
            default: self = .other
            }
        }
    }
}
