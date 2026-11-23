import Foundation

/// 到服务器的一条连接。测试里换成假的。
public protocol RelayConnection: Sendable {
    func send(_ text: String) async throws
    /// 下一条消息；断开时抛 `RelayClosed`。
    func receive() async throws -> String
    func close()
}

public protocol RelayTransport: Sendable {
    func open(server: URL, token: String) async throws -> any RelayConnection
}

public struct RelayClosed: Error, Sendable {
    /// WebSocket 关闭码：4401 = 登录失效，4000 = 同一台设备在别处连上了。
    public let code: Int

    public init(code: Int) { self.code = code }
}

/// 服务器转发协议里，服务器发来的消息（server/src/relay.ts）。
enum RelayIncoming: Decodable {
    case hello(peers: [PeerInfo])
    case message(id: String, from: String, body: String)
    case sent(id: String)
    case presence(device: String, online: Bool)
    case error(code: String, id: String?)
    case other

    private enum Keys: String, CodingKey { case type, peers, id, from, body, device, online, code }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "hello": self = .hello(peers: try c.decode([PeerInfo].self, forKey: .peers))
        case "message":
            self = .message(
                id: try c.decode(String.self, forKey: .id), from: try c.decode(String.self, forKey: .from), body: try c.decode(String.self, forKey: .body))
        case "sent": self = .sent(id: try c.decode(String.self, forKey: .id))
        case "presence": self = .presence(device: try c.decode(String.self, forKey: .device), online: try c.decode(Bool.self, forKey: .online))
        case "error": self = .error(code: try c.decode(String.self, forKey: .code), id: try c.decodeIfPresent(String.self, forKey: .id))
        default: self = .other
        }
    }
}

/// 手机发给服务器的。
enum RelayOutgoing: Encodable {
    case send(to: String, id: String, body: String)
    case ack([String])
    case ping

    private enum Keys: String, CodingKey { case type, to, id, body, ids }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .send(let to, let id, let body):
            try c.encode("send", forKey: .type)
            try c.encode(to, forKey: .to)
            try c.encode(id, forKey: .id)
            try c.encode(body, forKey: .body)
        case .ack(let ids):
            try c.encode("ack", forKey: .type)
            try c.encode(ids, forKey: .ids)
        case .ping: try c.encode("ping", forKey: .type)
        }
    }
}

/// 第 n 次重连前等多久（毫秒）：翻倍，最多 30 秒。实际再乘 0.5～1，免得服务器恢复时一起涌上来。
public enum Backoff {
    public static func delay(_ attempt: Int) -> Int64 { min(30_000, 1000 << min(max(attempt, 0), 5)) }
    public static func jittered(_ attempt: Int) -> Duration { .milliseconds(Int64(Double(delay(attempt)) * .random(in: 0.5...1))) }
}

/// 系统自带的 WebSocket。
public struct URLSessionTransport: RelayTransport {
    private let session: URLSession

    public init(session: URLSession = URLSession(configuration: .ephemeral)) {
        self.session = session
    }

    public func open(server: URL, token: String) async throws -> any RelayConnection {
        guard var parts = URLComponents(url: server.appending(path: "/v1/connect"), resolvingAgainstBaseURL: false) else { throw AppError.network }
        parts.scheme = parts.scheme == "http" ? "ws" : "wss"
        guard let url = parts.url else { throw AppError.network }
        var request = URLRequest(url: url, timeoutInterval: 20)
        // 令牌放请求头，不放地址里，免得进日志。
        request.setValue("Bearer \(token)", forHTTPHeaderField: "authorization")
        let task = session.webSocketTask(with: request)
        task.maximumMessageSize = 1024 * 1024
        task.resume()
        return Connection(task: task)
    }

    private struct Connection: RelayConnection {
        let task: URLSessionWebSocketTask

        func send(_ text: String) async throws {
            do { try await task.send(.string(text)) } catch { throw RelayClosed(code: task.closeCode.rawValue) }
        }

        func receive() async throws -> String {
            do {
                switch try await task.receive() {
                case .string(let text): return text
                case .data(let data): return String(decoding: data, as: UTF8.self)
                @unknown default: return ""
                }
            } catch {
                throw RelayClosed(code: task.closeCode.rawValue)
            }
        }

        func close() { task.cancel(with: .goingAway, reason: nil) }
    }
}
