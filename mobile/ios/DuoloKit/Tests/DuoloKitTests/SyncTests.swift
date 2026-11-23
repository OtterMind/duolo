import CryptoKit
import Foundation
import Testing

@testable import DuoloKit

/// 假的服务器和电脑：手机发来的记下来，想让手机收到什么就 `push`。
final class FakeRelay: RelayTransport, RelayConnection, @unchecked Sendable {
    private let lock = NSLock()
    private var inbox: [Result<String, RelayClosed>] = []
    private var waiter: CheckedContinuation<String, any Error>?
    private var sentFrames: [JSONValue] = []
    private var opens = 0
    private var greeted = 0
    var refuse = false

    var openCount: Int { lock.withLock { opens } }
    var sent: [JSONValue] { lock.withLock { sentFrames } }

    func open(server: URL, token: String) async throws -> any RelayConnection {
        lock.withLock {
            opens += 1
            inbox = []
        }
        if refuse { throw RelayClosed(code: 0) }
        return self
    }

    func send(_ text: String) async throws {
        let frame = try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))
        lock.withLock { sentFrames.append(frame) }
    }

    func receive() async throws -> String {
        try await withCheckedThrowingContinuation { c in
            lock.withLock {
                if inbox.isEmpty { waiter = c } else { c.resume(with: inbox.removeFirst().mapError { $0 as any Error }) }
            }
        }
    }

    /// 等手机连上（上次打过招呼之后新的一条连接）。
    func waitForConnection() async throws {
        try await until("a new connection") { lock.withLock { opens > greeted } }
        lock.withLock { greeted = opens }
    }

    func close() { deliver(.failure(RelayClosed(code: 1001))) }
    func drop(_ code: Int) { deliver(.failure(RelayClosed(code: code))) }

    func push(_ frame: JSONValue) {
        deliver(.success(String(decoding: try! JSONEncoder().encode(frame), as: UTF8.self)))
    }

    private func deliver(_ result: Result<String, RelayClosed>) {
        lock.withLock {
            if let c = waiter {
                waiter = nil
                c.resume(with: result.mapError { $0 as any Error })
            } else {
                inbox.append(result)
            }
        }
    }
}

/// 手机和假电脑的一整套。
struct Rig {
    let relay = FakeRelay()
    let store: LocalStore
    let engine: SyncEngine
    let computer: Computer
    let toPhone: SymmetricKey
    let fromPhone: SymmetricKey

    init(timing: (inout SyncEngine.Timing) -> Void = { _ in }, store existing: LocalStore? = nil) throws {
        let desktop = Curve25519.KeyAgreement.PrivateKey()
        let phone = DeviceKey()
        let secret = Data((0..<32).map { UInt8($0) })
        store = try existing ?? LocalStore(path: nil)
        computer = Computer(
            id: "mac", name: "MacBook Pro", platform: "mac", server: URL(string: "https://relay.test")!, deviceId: "phone",
            desktopKey: desktop.publicKey.rawRepresentation)
        try store.add(computer, now: 0)
        var t = SyncEngine.Timing()
        t.backoff = { _ in .milliseconds(5) }
        timing(&t)
        engine = try SyncEngine(
            computer: computer, secrets: Secrets(token: "t", privateKey: phone.privateKey, secret: secret), store: store, transport: relay,
            timing: t, now: { 1000 })
        let shared = try desktop.sharedSecretFromKeyAgreement(with: .init(rawRepresentation: phone.publicKey))
        func key(_ d: String) -> SymmetricKey {
            shared.hkdfDerivedSymmetricKey(
                using: SHA256.self, salt: secret,
                sharedInfo: Data("duolo/v1 \(d)".utf8) + desktop.publicKey.rawRepresentation + phone.publicKey, outputByteCount: 32)
        }
        toPhone = key("desktop->phone")
        fromPhone = key("phone->desktop")
    }

    func hello(online: Bool) async throws {
        try await relay.waitForConnection()
        relay.push(.object([
            "type": .string("hello"),
            "peers": .array([
                .object(["id": .string("mac"), "name": .string("MacBook Pro"), "platform": .string("mac"), "online": .bool(online)])
            ]),
        ]))
    }

    /// 电脑发来一条消息。
    func desktop(_ json: String, id: String = UUID().uuidString) {
        let aad = Data("duolo/v1\n\(id)\nmac\nphone".utf8)
        let box = try! ChaChaPoly.seal(Data(json.utf8), using: toPhone, authenticating: aad)
        raw(id, (Data([1]) + box.combined).base64EncodedString())
    }

    func raw(_ id: String, _ body: String) {
        relay.push(.object(["type": .string("message"), "id": .string(id), "from": .string("mac"), "body": .string(body), "at": .string("")]))
    }

    /// 手机发给电脑的，解开之后的内容。
    var received: [JSONValue] {
        relay.sent.filter { $0["type"].string == "send" }.map { f in
            let data = Data(base64Encoded: f["body"].string)!
            let aad = Data("duolo/v1\n\(f["id"].string)\nphone\nmac".utf8)
            let plain = try! ChaChaPoly.open(.init(combined: data.dropFirst()), using: fromPhone, authenticating: aad)
            return try! JSONDecoder().decode(JSONValue.self, from: plain)
        }
    }

    var acked: [String] { relay.sent.filter { $0["type"].string == "ack" }.flatMap { $0["ids"].array.map(\.string) } }

    func online() async throws {
        await engine.start()
        try await hello(online: true)
        try await until("online") { await engine.link == .connected(computerOnline: true) }
    }
}

func until(_ what: String, _ done: () async throws -> Bool) async throws {
    for _ in 0..<400 {
        if try await done() { return }
        try await Task.sleep(for: .milliseconds(5))
    }
    Issue.record("timed out waiting for \(what)")
    throw CancellationError()
}

func overview(rev: Int, tasks: [(id: Int, seq: Int)]) -> String {
    let list = tasks.map {
        #"{"id":\#($0.id),"project":"shop","title":"T","status":"running","writer":{"kind":"claude","name":"Claude"},"reviewer":{"kind":"codex","name":"Codex"},"updatedAt":1,"seq":\#($0.seq)}"#
    }
    return #"{"v":1,"type":"overview","rev":\#(rev),"tasks":[\#(list.joined(separator: ","))]}"#
}

func events(_ range: ClosedRange<Int>, latest: Int, task: Int = 3) -> String {
    let list = range.map { #"{"task":\#(task),"seq":\#($0),"at":1,"type":"message","speaker":"agent","agent":0,"text":"m\#($0)"}"# }
    return #"{"v":1,"type":"events","task":\#(task),"events":[\#(list.joined(separator: ","))],"latest":\#(latest)}"#
}

@Suite(.timeLimit(.minutes(1))) struct SyncTests {
    @Test func asksForAnOverviewWhenTheComputerIsOnline() async throws {
        let rig = try Rig()
        try await rig.online()
        try await until("sync request") { rig.received.contains { $0["type"].string == "sync" } }
        rig.desktop(overview(rev: 1, tasks: [(3, 0)]), id: "o1")
        try await until("ack") { rig.acked.contains("o1") }
        #expect(try rig.store.overview("mac")?.tasks.map(\.id) == [3])
    }

    @Test func doesNotQueueRequestsForAComputerThatIsOffline() async throws {
        let rig = try Rig()
        await rig.engine.start()
        try await rig.hello(online: false)
        try await until("connected") { await rig.engine.link == .connected(computerOnline: false) }
        try await Task.sleep(for: .milliseconds(50))
        #expect(rig.received.isEmpty)
        rig.relay.push(.object(["type": .string("presence"), "device": .string("mac"), "online": .bool(true)]))
        try await until("sync request") { rig.received.contains { $0["type"].string == "sync" } }
    }

    @Test func olderOverviewsAreIgnored() async throws {
        let rig = try Rig()
        try await rig.online()
        rig.desktop(overview(rev: 5, tasks: [(3, 0)]), id: "new")
        rig.desktop(overview(rev: 4, tasks: [(9, 0)]), id: "old")
        try await until("acks") { rig.acked.contains("new") && rig.acked.contains("old") }
        #expect(try rig.store.overview("mac")?.tasks.map(\.id) == [3])
    }

    @Test func eventsSurviveDuplicatesGapsAndPaging() async throws {
        let rig = try Rig()
        try await rig.online()
        rig.desktop(overview(rev: 1, tasks: [(3, 6)]), id: "o")
        try await until("overview") { rig.acked.contains("o") }
        await rig.engine.open(3)
        try await until("fetch") { rig.received.contains { $0["type"].string == "fetch" && $0["after"].int == 0 } }
        rig.desktop(events(1...2, latest: 6), id: "a")
        // 没到头，接着要。
        try await until("next page") { rig.received.contains { $0["after"].int == 2 } }
        rig.desktop(events(1...3, latest: 6), id: "dup")
        rig.desktop(events(6...6, latest: 6), id: "gap")
        try await until("refetch") { rig.received.contains { $0["after"].int == 3 } }
        #expect(try rig.store.lastSeq("mac", 3) == 3, "跳号的不能存")
        rig.desktop(events(4...6, latest: 6), id: "rest")
        try await until("all") { try rig.store.lastSeq("mac", 3) == 6 }
        #expect(try rig.store.events("mac", 3).map(\.seq) == Array(1...6))
    }

    @Test func startsOverWhenTheComputerLostItsHistory() async throws {
        let rig = try Rig()
        try await rig.online()
        await rig.engine.open(3)
        rig.desktop(events(1...9, latest: 9), id: "a")
        try await until("first") { try rig.store.lastSeq("mac", 3) == 9 }
        rig.desktop(events(1...2, latest: 2), id: "b")
        try await until("reset") { try rig.store.lastSeq("mac", 3) == 2 }
    }

    @Test func closedTasksAreForgotten() async throws {
        let rig = try Rig()
        try await rig.online()
        await rig.engine.open(3)
        rig.desktop(overview(rev: 1, tasks: [(3, 2)]), id: "o1")
        rig.desktop(events(1...2, latest: 2), id: "e")
        try await until("events") { try rig.store.lastSeq("mac", 3) == 2 }
        rig.desktop(overview(rev: 2, tasks: []), id: "o2")
        try await until("gone") { try rig.store.lastSeq("mac", 3) == 0 }
    }

    @Test func tamperedMessagesAreDroppedAndAcknowledged() async throws {
        let rig = try Rig()
        try await rig.online()
        rig.raw("bad", "AQAAAA==")
        rig.desktop(overview(rev: 1, tasks: [(3, 0)]), id: "good")
        try await until("both acked") { rig.acked.contains("bad") && rig.acked.contains("good") }
        #expect(try rig.store.overview("mac") != nil)
    }

    @Test func unknownMessagesDoNotStopTheStream() async throws {
        let rig = try Rig()
        try await rig.online()
        rig.desktop(#"{"v":1,"type":"hologram"}"#, id: "x")
        rig.desktop(#"not json"#, id: "y")
        rig.desktop(overview(rev: 1, tasks: [(3, 0)]), id: "good")
        try await until("acked") { Set(["x", "y", "good"]).isSubset(of: rig.acked) }
    }

    @Test func messagesSentOfflineGoOutOnceWithTheSameId() async throws {
        let rig = try Rig()
        try await rig.engine.submit(.sendMessage(task: 3, text: "hi"))
        let id = try rig.store.outbox("mac")[0].id
        await rig.engine.start()
        try await rig.hello(online: false)
        try await until("sent") { rig.relay.sent.contains { $0["id"].string == id } }
        // 还没收到 sent 就断了：重连后用同一个编号再发。
        rig.relay.drop(1006)
        try await until("reconnect") { rig.relay.openCount == 2 }
        try await rig.hello(online: false)
        try await until("resent") { rig.relay.sent.filter { $0["id"].string == id }.count == 2 }
        rig.relay.push(.object(["type": .string("sent"), "id": .string(id)]))
        try await until("marked") { try rig.store.outbox("mac")[0].state == .sent }
        // 服务器收下了：再重连不重发，等电脑的结果。
        rig.relay.drop(1006)
        try await until("reconnect again") { rig.relay.openCount == 3 }
        try await rig.hello(online: true)
        rig.desktop(#"{"v":1,"type":"result","id":"\#(id)","result":{"ok":true}}"#)
        try await until("done") { try rig.store.outbox("mac").isEmpty }
        #expect(rig.relay.sent.filter { $0["id"].string == id }.count == 2)
    }

    @Test func rejectedCommandsStayVisibleAndCanBeRetried() async throws {
        let rig = try Rig()
        try await rig.online()
        try await rig.engine.submit(.startReview(task: 3))
        let id = try rig.store.outbox("mac")[0].id
        rig.desktop(#"{"v":1,"type":"result","id":"\#(id)","result":{"ok":false,"error":"taskNotFound"}}"#)
        try await until("failed") { try rig.store.outbox("mac").first?.state == .failed }
        #expect(try rig.store.outbox("mac")[0].error == "taskNotFound")
        try await rig.engine.retry(id)
        let again = try rig.store.outbox("mac")
        #expect(again.count == 1 && again[0].id != id && again[0].state == .queued)
    }

    @Test func answersAreNeverQueued() async throws {
        let rig = try Rig()
        await #expect(throws: AppError.computerOffline) { try await rig.engine.submit(.answer(task: 3, option: 0, prompt: 7)) }
        #expect(try rig.store.outbox("mac").isEmpty)

        try await rig.online()
        async let answer: Void = rig.engine.submit(.answer(task: 3, option: 0, prompt: 7))
        try await until("answer sent") { rig.received.contains { $0["command"]["prompt"].int == 7 } }
        let id = rig.received.first { $0["type"].string == "command" }!["id"].string
        rig.desktop(#"{"v":1,"type":"result","id":"\#(id)","result":{"ok":false,"error":"promptChanged"}}"#)
        do {
            try await answer
            Issue.record("should fail")
        } catch {
            #expect(error as? AppError == AppError("promptChanged"))
        }
        #expect(try rig.store.outbox("mac").isEmpty)
    }

    @Test func answersFailWhenTheConnectionDropsOrNobodyReplies() async throws {
        let rig = try Rig { $0.reply = .milliseconds(80) }
        try await rig.online()
        await #expect(throws: AppError.network) { try await rig.engine.submit(.interrupt(task: 3)) }

        let slow = try Rig()
        try await slow.online()
        async let answer: Void = slow.engine.submit(.interrupt(task: 3))
        try await until("sent") { slow.received.contains { $0["type"].string == "command" } }
        slow.relay.drop(1006)
        do {
            try await answer
            Issue.record("should fail")
        } catch { #expect(error as? AppError == .network) }
    }

    @Test func stopsWhenUnlinked() async throws {
        let rig = try Rig()
        try await rig.online()
        rig.relay.drop(4401)
        try await until("unlinked") { await rig.engine.link == .unlinked }
        try await Task.sleep(for: .milliseconds(50))
        #expect(rig.relay.openCount == 1, "登录失效后不再重连")
    }

    @Test func keepsReconnecting() async throws {
        let rig = try Rig()
        rig.relay.refuse = true
        await rig.engine.start()
        try await until("several attempts") { rig.relay.openCount >= 4 }
        rig.relay.refuse = false
        let before = rig.relay.openCount
        try await until("reopened") { rig.relay.openCount > before }
        try await rig.hello(online: true)
        try await until("online") { await rig.engine.link == .connected(computerOnline: true) }
        await rig.engine.stop()
    }

    @Test func noticesADeadConnection() async throws {
        let rig = try Rig {
            $0.ping = .milliseconds(20)
            $0.silence = .milliseconds(60)
        }
        try await rig.online()
        try await until("ping") { rig.relay.sent.contains { $0["type"].string == "ping" } }
        try await until("reconnect") { rig.relay.openCount >= 2 }
        await rig.engine.stop()
    }

    @Test func dataSurvivesARestart() async throws {
        let path = FileManager.default.temporaryDirectory.appending(path: "duolo-\(UUID().uuidString).sqlite").path
        defer { try? FileManager.default.removeItem(atPath: path) }
        do {
            let rig = try Rig(store: LocalStore(path: path))
            try await rig.engine.submit(.sendMessage(task: 3, text: "hi"))
            try await rig.online()
            await rig.engine.open(3)
            rig.desktop(events(1...2, latest: 2), id: "e")
            try await until("events") { try rig.store.lastSeq("mac", 3) == 2 }
            await rig.engine.stop()
        }
        let again = try LocalStore(path: path)
        #expect(try again.computers().map(\.id) == ["mac"])
        #expect(try again.events("mac", 3).count == 2)
        #expect(try again.outbox("mac").count == 1)
        try again.remove("mac")
        #expect(try again.events("mac", 3).isEmpty && again.outbox("mac").isEmpty, "断开后全部清掉")
    }

    @Test func backoffMatchesTheContract() throws {
        let delays = try Repo.json("mobile/contract/behavior.json")["backoff"]["delays"].array.compactMap(\.int)
        #expect(delays.indices.map { Backoff.delay($0) } == delays)
    }
}
