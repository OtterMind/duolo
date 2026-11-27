import Foundation
import Security

/// 一台电脑的机密：登录令牌、这台手机的私钥、配对时的一次性密码。
public struct Secrets: Codable, Hashable, Sendable {
    public let token: String
    public let privateKey: Data
    public let secret: Data

    public init(token: String, privateKey: Data, secret: Data) {
        self.token = token
        self.privateKey = privateKey
        self.secret = secret
    }
}

public protocol SecretStore: Sendable {
    func load(_ computer: String) throws -> Secrets?
    func save(_ secrets: Secrets, for computer: String) throws
    func remove(_ computer: String) throws
}

/// 钥匙串。只在这台手机上，不随备份和 iCloud 走；解锁过一次之后后台也能用。
public struct Keychain: SecretStore {
    public struct Failure: Error { public let status: OSStatus }

    private let service: String

    public init(service: String = "io.duolo.app.pairing") {
        self.service = service
    }

    private func query(_ computer: String) -> [CFString: Any] {
        [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: computer]
    }

    public func load(_ computer: String) throws -> Secrets? {
        var q = query(computer)
        q[kSecReturnData] = true
        var out: CFTypeRef?
        switch SecItemCopyMatching(q as CFDictionary, &out) {
        case errSecSuccess: return try (out as? Data).map { try JSONDecoder().decode(Secrets.self, from: $0) }
        case errSecItemNotFound: return nil
        case let status: throw Failure(status: status)
        }
    }

    public func save(_ secrets: Secrets, for computer: String) throws {
        let data = try JSONEncoder().encode(secrets)
        let attributes: [CFString: Any] = [
            kSecValueData: data, kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = SecItemUpdate(query(computer) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            status = SecItemAdd(query(computer).merging(attributes) { $1 } as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw Failure(status: status) }
    }

    public func remove(_ computer: String) throws {
        let status = SecItemDelete(query(computer) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw Failure(status: status) }
    }
}

/// 测试用。
public final class MemorySecrets: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String: Secrets] = [:]

    public init() {}
    public func load(_ computer: String) -> Secrets? { lock.withLock { items[computer] } }
    public func save(_ secrets: Secrets, for computer: String) { lock.withLock { items[computer] = secrets } }
    public func remove(_ computer: String) { lock.withLock { items[computer] = nil } }
}
