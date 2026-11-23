import CryptoKit
import Foundation

/// 和一台电脑之间的端到端加密，算法见 docs/mobile.md。
///
/// 密钥由两边的 X25519 密钥和二维码里的一次性密码一起算出。密码不经过服务器，
/// 所以服务器即使换掉公钥也算不出密钥，既看不到内容也冒充不了任何一边。
public struct SecureChannel: Sendable {
    public enum Failure: Error { case badKey, badMessage }

    let sendKey: SymmetricKey
    let receiveKey: SymmetricKey

    public init(privateKey: Data, desktopKey: Data, secret: Data) throws {
        guard secret.count == 32, desktopKey.count == 32,
            let mine = try? Curve25519.KeyAgreement.PrivateKey(rawRepresentation: privateKey),
            let theirs = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: desktopKey),
            let shared = try? mine.sharedSecretFromKeyAgreement(with: theirs)
        else { throw Failure.badKey }
        // 小阶点会算出全 0 的共享密钥，等于没有加密。
        guard shared.withUnsafeBytes({ $0.contains { $0 != 0 } }) else { throw Failure.badKey }
        func key(_ direction: String) -> SymmetricKey {
            shared.hkdfDerivedSymmetricKey(
                using: SHA256.self, salt: secret,
                sharedInfo: Data("duolo/v1 \(direction)".utf8) + desktopKey + mine.publicKey.rawRepresentation,
                outputByteCount: 32)
        }
        sendKey = key("phone->desktop")
        receiveKey = key("desktop->phone")
    }

    /// 加密后的 base64：版本(1) + nonce(12) + 密文 + 校验(16)。
    public func seal(_ plaintext: Data, id: String, from: String, to: String) throws -> String {
        try seal(plaintext, id: id, from: from, to: to, nonce: ChaChaPoly.Nonce())
    }

    func seal(_ plaintext: Data, id: String, from: String, to: String, nonce: ChaChaPoly.Nonce) throws -> String {
        let box = try ChaChaPoly.seal(plaintext, using: sendKey, nonce: nonce, authenticating: Self.header(id, from, to))
        return (Data([1]) + box.combined).base64EncodedString()
    }

    public func open(_ body: String, id: String, from: String, to: String) throws -> Data {
        guard let data = Data(base64Encoded: body), data.count > 29, data.first == 1,
            let box = try? ChaChaPoly.SealedBox(combined: data.dropFirst()),
            let plaintext = try? ChaChaPoly.open(box, using: receiveKey, authenticating: Self.header(id, from, to))
        else { throw Failure.badMessage }
        return plaintext
    }

    /// 编号和收发双方一起校验：服务器不能把一条消息改个编号或换个对象再送一次。
    private static func header(_ id: String, _ from: String, _ to: String) -> Data {
        Data("duolo/v1\n\(id)\n\(from)\n\(to)".utf8)
    }
}

/// 这台手机的一对密钥，每次配对新生成。
public struct DeviceKey: Sendable {
    public let privateKey: Data
    public let publicKey: Data

    public init() {
        let key = Curve25519.KeyAgreement.PrivateKey()
        privateKey = key.rawRepresentation
        publicKey = key.publicKey.rawRepresentation
    }
}
