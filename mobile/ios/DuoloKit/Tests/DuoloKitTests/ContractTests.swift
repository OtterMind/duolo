import CryptoKit
import Foundation
import Testing

@testable import DuoloKit

/// 仓库里的文件。三端共用的样例在 mobile/contract/。
enum Repo {
    static let root = URL(filePath: #filePath).appending(path: "../../../../../..").standardizedFileURL

    static func json(_ path: String) throws -> JSONValue {
        try JSONDecoder().decode(JSONValue.self, from: Data(contentsOf: root.appending(path: path)))
    }
}

extension JSONValue {
    subscript(_ key: String) -> JSONValue {
        if case .object(let o) = self { return o[key] ?? .null }
        return .null
    }
    var array: [JSONValue] { if case .array(let a) = self { a } else { [] } }
    var string: String { if case .string(let s) = self { s } else { "" } }
    var int: Int64? { if case .int(let i) = self { i } else { nil } }
    var bool: Bool { self == .bool(true) }

    func decoded<T: Decodable>(_ type: T.Type) throws -> T {
        try JSONDecoder().decode(type, from: JSONEncoder().encode(self))
    }
    init(encoding value: some Encodable) throws {
        self = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(value))
    }
}

@Suite struct WireContract {
    let contract = try! Repo.json("mobile/contract/wire.json")

    @Test func phoneMessagesMatchTheDesktop() throws {
        for c in contract["phone"].array {
            let message = try c["json"].decoded(PhoneMessage.self)
            #expect(try JSONValue(encoding: message) == c["json"], "\(c["name"].string)")
        }
    }

    @Test func desktopMessagesAreUnderstood() throws {
        var kinds: [String] = []
        for c in contract["desktop"].array {
            switch try c["json"].decoded(DesktopMessage.self) {
            case .overview(let o):
                kinds.append("overview")
                #expect(o.rev == 7 && o.tasks.count == 2 && o.projects[0].branches == ["main", "develop"])
                #expect(o.tasks[0].prompt?.seq == 42 && o.tasks[0].diff?.insertions == 96)
                #expect(o.tasks[1].review?.issues == 2)
            case .events(let task, let events, let latest):
                kinds.append("events")
                #expect(task == 3 && latest == 11 && events.map(\.seq) == Array(1...11))
                #expect(!events.contains { $0.kind == .unknown })
                #expect(events[6].kind == .prompt(question: "Proceed?", detail: "pnpm test", options: ["Yes", "No"]))
                // 保存的是原样。
                #expect(try JSONValue(encoding: events) == c["json"]["events"])
            case .review(let task, let at, let output):
                kinds.append("review")
                #expect(task == 4 && at == 1_795_361_000_000 && output.hasPrefix("[ISSUES 2]"))
            case .result(let id, let result):
                kinds.append("result")
                #expect(result.ok ? (id == "c-1" && result.task == 9) : result.error == "promptChanged")
            case .unknown:
                Issue.record("\(c["name"].string) was not understood")
            }
        }
        #expect(kinds == ["overview", "events", "review", "result", "result"])
    }

    @Test func newerDesktopsDoNotBreakOldPhones() throws {
        let cases = contract["tolerated"].array
        #expect(try cases[0]["json"].decoded(DesktopMessage.self) == .unknown)
        guard case .events(_, let events, _) = try cases[1]["json"].decoded(DesktopMessage.self) else {
            Issue.record("not events")
            return
        }
        #expect(events[0].kind == .unknown && events[1].kind == .message(fromUser: false, text: "hi"))
        #expect(try JSONValue(encoding: events[0])["shape"].string == "cube", "不认识的事件原样保留")
    }
}

@Suite struct CryptoContract {
    let contract = try! Repo.json("mobile/contract/crypto.json")
    func data(_ v: JSONValue) -> Data { Data(base64Encoded: v.string)! }

    @Test func matchesTheReferenceVectors() throws {
        let channel = try SecureChannel(
            privateKey: data(contract["phone"]["privateKey"]), desktopKey: data(contract["desktop"]["publicKey"]),
            secret: data(contract["secret"]))
        for m in contract["messages"].array {
            let (id, from, to) = (m["id"].string, m["from"].string, m["to"].string)
            let plaintext = Data(m["plaintext"].string.utf8)
            if m["direction"].string == "phoneToDesktop" {
                let body = try channel.seal(plaintext, id: id, from: from, to: to, nonce: .init(data: data(m["nonce"])))
                #expect(body == m["body"].string)
            } else {
                #expect(try channel.open(m["body"].string, id: id, from: from, to: to) == plaintext)
            }
        }
    }

    @Test func rejectsAnythingChanged() throws {
        let channel = try SecureChannel(
            privateKey: data(contract["phone"]["privateKey"]), desktopKey: data(contract["desktop"]["publicKey"]),
            secret: data(contract["secret"]))
        let m = contract["messages"].array[1]
        let body = m["body"].string
        func fails(_ body: String, id: String = "m-2", from: String = "desktop-1", to: String = "phone-1") -> Bool {
            (try? channel.open(body, id: id, from: from, to: to)) == nil
        }
        #expect(!fails(body))
        #expect(fails(body, id: "m-9"), "换编号重放")
        #expect(fails(body, from: "desktop-2") && fails(body, to: "phone-2"))
        #expect(fails("") && fails("AQ==") && fails("not base64"))
        var bytes = Data(base64Encoded: body)!
        bytes[bytes.count - 1] ^= 1
        #expect(fails(bytes.base64EncodedString()))
        bytes = Data(base64Encoded: body)!
        bytes[0] = 2
        #expect(fails(bytes.base64EncodedString()), "不认识的版本")
    }

    @Test func wrongSecretMeansADifferentKey() throws {
        let other = try SecureChannel(
            privateKey: data(contract["phone"]["privateKey"]), desktopKey: data(contract["desktop"]["publicKey"]),
            secret: Data(repeating: 9, count: 32))
        let m = contract["messages"].array[1]
        #expect(throws: SecureChannel.Failure.self) {
            try other.open(m["body"].string, id: "m-2", from: "desktop-1", to: "phone-1")
        }
    }

    @Test func rejectsBadKeys() {
        let mine = DeviceKey()
        #expect(throws: (any Error).self) {
            try SecureChannel(privateKey: mine.privateKey, desktopKey: Data(count: 32), secret: Data(repeating: 1, count: 32))
        }
        #expect(throws: (any Error).self) {
            try SecureChannel(privateKey: mine.privateKey, desktopKey: DeviceKey().publicKey, secret: Data(count: 8))
        }
    }
}

@Suite struct BehaviorContract {
    let contract = try! Repo.json("mobile/contract/behavior.json")

    /// 样例里只写关心的字段，其余补默认值。
    func task(_ partial: JSONValue) throws -> TaskOverview {
        guard case .object(var o) = partial else { throw CocoaError(.coderInvalidValue) }
        let agent: JSONValue = .object(["kind": .string("claude"), "name": .string("Claude")])
        let defaults: [String: JSONValue] = [
            "id": .int(1), "project": .string("shop"), "title": .string("t"), "writer": agent, "reviewer": agent, "updatedAt": .int(0),
        ]
        o.merge(defaults) { mine, _ in mine }
        return try JSONValue.object(o).decoded(TaskOverview.self)
    }

    @Test func previews() {
        for c in contract["preview"].array { #expect(Excerpt.first(c["in"].string) == c["out"].string, "\(c["in"].string)") }
    }

    @Test func rows() throws {
        for c in contract["rows"].array {
            // 状态由电脑算好（Rust 的测试核对规则），这里核对分组和第二行。
            guard case .object(var fields) = c["task"] else { continue }
            fields["state"] = c["state"]
            let row = TaskRow(try task(.object(fields)))
            #expect(row.state.code == c["state"].string, "\(c["name"].string)")
            #expect(row.group.rawValue == c["group"].string, "\(c["name"].string)")
            #expect(row.detail == c["detail"].string, "\(c["name"].string)")
        }
    }

    @Test func order() throws {
        let tasks = try contract["sort"]["tasks"].array.map(task)
        #expect(TaskRow.rows(tasks).map(\.id) == contract["sort"]["order"].array.compactMap(\.int))
    }

    @Test func timelines() throws {
        for c in contract["timeline"].array {
            let t = Timeline(try c["events"].decoded([TaskEvent].self))
            let items = t.items.map { item -> JSONValue in
                switch item {
                case .user(let seq, let text): .object(["kind": .string("user"), "seq": .int(seq), "text": .string(text)])
                case .agent(let seq, let text): .object(["kind": .string("agent"), "seq": .int(seq), "text": .string(text)])
                case .steps(let steps):
                    .object({
                        let n = Timeline.Counts(steps)
                        return [
                            "kind": .string("steps"), "seq": .int(item.id), "read": .int(Int64(n.read)), "edit": .int(Int64(n.edit)),
                            "run": .int(Int64(n.run)), "other": .int(Int64(n.other)),
                            "steps": .array(steps.map { .string("\($0.kind.rawValue) \($0.target)") }),
                        ]
                    }())
                case .review(let seq, let reviewer, let ok, let verdict, let issues, let summary):
                    .object([
                        "kind": .string("review"), "seq": .int(seq), "reviewer": .string(reviewer), "ok": .bool(ok),
                        "verdict": .string(verdict), "issues": .int(Int64(issues)), "summary": .string(summary),
                    ])
                }
            }
            #expect(items == c["items"].array, "\(c["name"].string)")
            #expect(t.activity.map { "\($0.kind.rawValue) \($0.target)" } ?? "" == c["activity"].string)
            #expect((t.diff.map { [$0.files, $0.insertions, $0.deletions].map { Int64($0) } } ?? []) == c["diff"].array.compactMap(\.int))
            #expect(t.closed == c["closed"].bool)
        }
    }

    @Test func pairingLinks() {
        for c in contract["pairing"]["valid"].array {
            let link = PairingLink(c["link"].string)
            #expect(link?.server.absoluteString == c["server"].string)
            #expect(link?.code == c["code"].string)
            #expect(link?.desktopKey.base64EncodedString() == c["key"].string)
            #expect(link?.secret.base64EncodedString() == c["secret"].string)
        }
        for c in contract["pairing"]["invalid"].array { #expect(PairingLink(c["link"].string) == nil, "\(c["why"].string)") }
    }

    @Test func locales() {
        for c in contract["locales"].array { #expect(Localizer.match(c["tags"].array.map(\.string)) == c["code"].string) }
    }

    @Test func everyLanguageHasEveryKey() throws {
        let dir = Repo.root.appending(path: "locales")
        let base = try JSONDecoder().decode([String: String].self, from: Data(contentsOf: dir.appending(path: "en.json")))
        for (code, _) in Localizer.locales {
            let t = Localizer(code: code, directory: dir)
            #expect(t("mobile.send") != "mobile.send", "\(code)")
            let mine = try JSONDecoder().decode([String: String].self, from: Data(contentsOf: dir.appending(path: "\(code).json")))
            #expect(Set(mine.keys) == Set(base.keys), "\(code)")
        }
        let zh = Localizer(code: "zh-CN", directory: dir)
        #expect(zh("state.issues", ["count": "2"]) == "2 个问题")
        #expect(zh("no.such.key") == "no.such.key")
        #expect(zh.error("promptChanged") != "promptChanged" && zh.error("weird").contains("weird"))
    }
}
