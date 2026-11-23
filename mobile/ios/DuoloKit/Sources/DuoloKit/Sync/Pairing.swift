import Foundation

/// 配对和解除。
public struct Pairing: Sendable {
    let store: LocalStore
    let secrets: any SecretStore
    let session: URLSession

    public init(store: LocalStore, secrets: any SecretStore, session: URLSession = .shared) {
        self.store = store
        self.secrets = secrets
        self.session = session
    }

    /// 用二维码里的内容连接电脑。
    public func connect(_ link: PairingLink, deviceName: String, platform: String, now: Int64) async throws -> Computer {
        let key = DeviceKey()
        let api = ServerAPI(server: link.server, session: session)
        let claim = try await api.claim(code: link.code, name: deviceName, platform: platform, publicKey: key.publicKey)
        // 公钥以二维码里的为准：服务器给的不一样，说明它换了一把。
        guard Data(base64Encoded: claim.desktop.publicKey) == link.desktopKey else {
            try? await api.unlink(device: claim.device.id, token: claim.token)
            throw AppError.badPairingLink
        }
        let computer = Computer(
            id: claim.desktop.id, name: claim.desktop.name, platform: claim.desktop.platform, server: link.server, deviceId: claim.device.id,
            desktopKey: link.desktopKey)
        do {
            // 先存机密：只有电脑没有机密的话，界面上会有一台连不上的电脑。
            try secrets.save(Secrets(token: claim.token, privateKey: key.privateKey, secret: link.secret), for: computer.id)
            try store.add(computer, now: now)
        } catch {
            throw AppError.storage
        }
        return computer
    }

    /// 断开一台电脑。服务器连不上也照样清掉手机上的。
    public func disconnect(_ computer: Computer) async throws {
        if let token = try? secrets.load(computer.id)?.token {
            try? await ServerAPI(server: computer.server, session: session).unlink(device: computer.deviceId, token: token)
        }
        do {
            try store.remove(computer.id)
            try secrets.remove(computer.id)
        } catch {
            throw AppError.storage
        }
    }
}
