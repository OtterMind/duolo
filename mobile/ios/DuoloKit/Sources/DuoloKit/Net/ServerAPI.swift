import Foundation

/// 给用户看的错误：错误码，文案是 `error.<code>`。
public struct AppError: Error, Hashable, Sendable {
    public let code: String

    public init(_ code: String) { self.code = code }

    public static let network = AppError("network")
    public static let server = AppError("server")
    public static let unauthorized = AppError("unauthorized")
    public static let computerOffline = AppError("computerOffline")
    public static let storage = AppError("storage")
    public static let badPairingLink = AppError("badPairingLink")
}

public struct PeerInfo: Decodable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let platform: String
    public let online: Bool
}

/// 服务端的 HTTP 接口（server/src/app.ts）。
public struct ServerAPI: Sendable {
    public struct Claim: Decodable, Sendable {
        public struct Desktop: Decodable, Sendable {
            public let id: String
            public let name: String
            public let platform: String
            public let publicKey: String
        }

        public let token: String
        public let device: PeerInfo
        public let desktop: Desktop
    }

    private struct ErrorBody: Decodable { let error: String }

    let server: URL
    let session: URLSession

    public init(server: URL, session: URLSession = .shared) {
        self.server = server
        self.session = session
    }

    /// 用配对码登录，登记这台手机。
    public func claim(code: String, name: String, platform: String, publicKey: Data) async throws -> Claim {
        struct Device: Encodable { let name, platform, publicKey: String }
        struct Body: Encodable { let code: String; let device: Device }
        let body = Body(code: code, device: Device(name: String(name.prefix(80)), platform: platform, publicKey: publicKey.base64EncodedString()))
        return try await call("POST", "/v1/pairings/claim", body: JSONEncoder().encode(body))
    }

    /// 解除这台手机：它的登录一起失效。
    public func unlink(device: String, token: String) async throws {
        struct Ok: Decodable {}
        let _: Ok = try await call("DELETE", "/v1/devices/\(device)", token: token)
    }

    private func call<T: Decodable>(_ method: String, _ path: String, token: String? = nil, body: Data? = nil) async throws -> T {
        var request = URLRequest(url: server.appending(path: path), timeoutInterval: 20)
        request.httpMethod = method
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "content-type")
        }
        if let token { request.setValue("Bearer \(token)", forHTTPHeaderField: "authorization") }
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw AppError.network
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let code = (try? JSONDecoder().decode(ErrorBody.self, from: data))?.error
            switch (status, code) {
            case (401, _): throw AppError.unauthorized
            case (_, "invalidPairingCode"): throw AppError("invalidPairingCode")
            default: throw AppError.server
            }
        }
        guard let value = try? JSONDecoder().decode(T.self, from: data) else { throw AppError.server }
        return value
    }
}
