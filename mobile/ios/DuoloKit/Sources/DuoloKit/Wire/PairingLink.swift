import Foundation

/// 电脑上二维码的内容：服务器地址、一次性配对码、电脑的公钥、一次性密码。
public struct PairingLink: Hashable, Sendable {
    public let server: URL
    public let code: String
    public let desktopKey: Data
    public let secret: Data

    public init?(_ text: String) {
        guard let parts = URLComponents(string: text.trimmingCharacters(in: .whitespacesAndNewlines)),
            parts.scheme?.lowercased() == "duolo", parts.host == "pair"
        else { return nil }
        func item(_ name: String) -> String? { parts.queryItems?.first { $0.name == name }?.value }
        guard item("v") == "1",
            let server = item("server").flatMap(Self.serverURL),
            let code = item("code")?.uppercased(), (4...32).contains(code.count),
            let key = item("key").flatMap(Data.init(base64URL:)), key.count == 32,
            let secret = item("secret").flatMap(Data.init(base64URL:)), secret.count == 32
        else { return nil }
        self.server = server
        self.code = code
        self.desktopKey = key
        self.secret = secret
    }

    /// 显示给用户确认的服务器名字。
    public var serverName: String {
        (server.host ?? server.absoluteString) + (server.port.map { ":\($0)" } ?? "")
    }

    /// 只要 https；http 只许连自己电脑或局域网里的（开发、自己架设）。
    static func serverURL(_ text: String) -> URL? {
        guard var parts = URLComponents(string: text), let scheme = parts.scheme?.lowercased(),
            let host = parts.host, !host.isEmpty
        else { return nil }
        guard scheme == "https" || (scheme == "http" && isLocal(host)) else { return nil }
        while parts.path.hasSuffix("/") { parts.path.removeLast() }
        parts.query = nil
        parts.fragment = nil
        return parts.url
    }

    static func isLocal(_ host: String) -> Bool {
        if host == "localhost" || host.hasSuffix(".local") { return true }
        let bytes = host.split(separator: ".", omittingEmptySubsequences: false).map { UInt8($0) }
        guard bytes.count == 4, let a = bytes[0], let b = bytes[1], bytes[2] != nil, bytes[3] != nil else { return false }
        return a == 127 || a == 10 || (a == 192 && b == 168) || (a == 172 && (16...31).contains(b))
    }
}

extension Data {
    init?(base64URL text: String) {
        var s = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while s.count % 4 != 0 { s += "=" }
        self.init(base64Encoded: s)
    }
}
