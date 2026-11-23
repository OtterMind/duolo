import Foundation
import Testing

@testable import DuoloKit

/// 和联调沙箱（server/dev/mobile-sandbox.ts）里真的服务端、真的加密走一遍。
/// 先启动沙箱，再 `DUOLO_SANDBOX=http://localhost:8788 swift test`；没设这个变量就跳过。
private let sandbox = ProcessInfo.processInfo.environment["DUOLO_SANDBOX"].flatMap(URL.init(string:))

@Suite(.serialized, .enabled(if: sandbox != nil), .timeLimit(.minutes(1))) struct SandboxTests {
    func control(_ path: String) async throws -> String {
        let (data, _) = try await URLSession.shared.data(from: sandbox!.appending(path: path))
        return String(decoding: data, as: UTF8.self)
    }

    func wait(_ what: String, _ done: () async throws -> Bool) async throws {
        for _ in 0..<200 {
            if try await done() { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        Issue.record("timed out waiting for \(what)")
        throw CancellationError()
    }

    @Test func pairsSyncsAnswersAndRecovers() async throws {
        let store = try LocalStore(path: nil)
        let secrets = MemorySecrets()
        let link = try #require(PairingLink(try await control("link")))
        let pairing = Pairing(store: store, secrets: secrets)
        let computer = try await pairing.connect(link, deviceName: "Test iPhone", platform: "ios", now: 1)
        // 配对码只能用一次。
        await #expect(throws: AppError("invalidPairingCode")) {
            try await pairing.connect(link, deviceName: "Again", platform: "ios", now: 2)
        }

        let engine = try SyncEngine(computer: computer, secrets: secrets.load(computer.id)!, store: store)
        await engine.start()
        try await wait("overview") { try (store.overview(computer.id)?.tasks.count ?? 0) >= 8 }
        let rows = TaskRow.rows(try #require(try store.overview(computer.id)).tasks)
        #expect(rows[0].state == .needsYou && rows.contains { $0.state == .issues(2) })
        #expect(rows.contains { $0.state == .reviewing } && rows.contains { $0.state == .writing })

        // 打开任务：事件补齐，审阅全文也来了。
        let deps = try #require(rows.first { $0.state == .issues(2) })
        await engine.open(deps.id)
        try await wait("events") { try store.lastSeq(computer.id, deps.id) == deps.task.seq }
        try await wait("review") { try store.review(computer.id, deps.id)?.output.contains("esbuild") == true }

        // 回答错的提示不执行；回答对的执行。
        let csv = rows[0]
        let prompt = try #require(csv.task.prompt)
        await #expect(throws: AppError("promptChanged")) { try await engine.submit(.answer(task: csv.id, option: 0, prompt: prompt.seq + 5)) }
        try await engine.submit(.answer(task: csv.id, option: 0, prompt: prompt.seq))
        await #expect(throws: AppError("promptChanged"), "同一条提示不能回答两次") {
            try await engine.submit(.answer(task: csv.id, option: 0, prompt: prompt.seq))
        }
        try await wait("running") {
            TaskRow(try #require(try store.overview(computer.id)?.tasks.first { $0.id == csv.id })).state == .writing
        }

        // 电脑离线：消息排队，回答直接报错；上线后送达。
        _ = try await control("offline")
        try await wait("offline") { await engine.link == .connected(computerOnline: false) }
        await #expect(throws: AppError.computerOffline) { try await engine.submit(.interrupt(task: csv.id)) }
        try await engine.submit(.sendMessage(task: deps.id, text: "Use Node 22 — 用 22 版 👋"))
        try await wait("relayed") { try store.outbox(computer.id).first?.state == .sent }
        _ = try await control("online")
        try await wait("delivered") { try store.outbox(computer.id).isEmpty }
        try await wait("message event") { try store.events(computer.id, deps.id).contains { $0.kind == .message(fromUser: true, text: "Use Node 22 — 用 22 版 👋") } }

        // 新建任务。
        try await engine.submit(.createTask(project: "shop", prompt: "Add dark mode", base: nil, writer: nil, reviewer: nil))
        try await wait("new task") { try store.overview(computer.id)?.tasks.contains { $0.title == "Add dark mode" } == true }

        // 断开后令牌失效，引擎停下。
        try await ServerAPI(server: computer.server).unlink(device: computer.deviceId, token: secrets.load(computer.id)!.token)
        try await wait("unlinked") { await engine.link == .unlinked }
        try await pairing.disconnect(computer)
        #expect(try store.computers().isEmpty && secrets.load(computer.id) == nil)
    }
}
