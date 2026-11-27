import Foundation
import Testing

@testable import DuoloKit

/// 语音：热词和对话提示（和电脑端 hints.rs 的测试同一组样例）、识别接口的地址和消息。
@Suite struct VoiceTests {
    private func event(_ json: String) throws -> TaskEvent {
        try JSONDecoder().decode(TaskEvent.self, from: Data(json.utf8))
    }

    @Test func filesAndCodeNamesNewestFirst() throws {
        let events = try [
            event(#"{"task":1,"seq":1,"at":0,"type":"tool","agent":1,"tool":"Read","target":"src/old.rs"}"#),
            event(#"{"task":1,"seq":2,"at":0,"type":"tool","agent":1,"tool":"Bash","target":"cargo test --workspace"}"#),
            event(#"{"task":1,"seq":3,"at":0,"type":"message","speaker":"user","agent":1,"text":"export orders from OrderList"}"#),
            event(#"{"task":1,"seq":4,"at":0,"type":"message","speaker":"agent","agent":1,"text":"I added `exportCsv` to `app/src/orders.ts`. Should I also handle `a b`?"}"#),
            event(#"{"task":1,"seq":5,"at":0,"type":"prompt","agent":1,"question":"Run tests?","detail":"","options":["Yes","No"]}"#),
        ]
        let hints = VoiceHints(project: "shop", title: "Export orders", branch: "export-csv", events: events)
        #expect(hints.hotwords == ["exportCsv", "app/src/orders.ts", "OrderList", "old.rs", "export-csv", "shop"])
        #expect(hints.context.first == "Run tests? (Yes / No)")
        #expect(hints.context[1].hasPrefix("I added"))
        #expect(hints.context[2] == "export orders from OrderList")
        #expect(hints.context.last == "Export orders")
    }

    @Test func codeNamesInPlainText() {
        #expect(VoiceHints.codeNames("Fix exportCsv in orders.ts (see duolo_core::t, https://a.com/x). Then run it.")
            == ["exportCsv", "orders.ts", "duolo_core::t"])
    }

    @Test func joinsNamesSaidInPieces() {
        let hints = VoiceHints(hotwords: ["useState", "exportCsv", "orders.ts", "Tauri"])
        #expect(hints.fixNames("加一个 use state 的 hook") == "加一个 useState 的 hook")
        #expect(hints.fixNames("把 Export CSV 改到 orders ts 里") == "把 exportCsv 改到 orders.ts 里")
        #expect(hints.fixNames("usestate and use_state_x") == "useState and use_state_x")
        // 只说了一段、已经对了、单个词的，不动。
        #expect(hints.fixNames("use it, useState, tauri") == "use it, useState, tauri")
    }

    @Test func voiceURLFollowsTheScheme() {
        #expect(VoiceStream.voiceURL(URL(string: "https://duolo.example/")!)?.absoluteString == "wss://duolo.example/v1/voice")
        #expect(VoiceStream.voiceURL(URL(string: "http://localhost:8787")!)?.absoluteString == "ws://localhost:8787/v1/voice")
        #expect(VoiceStream.voiceURL(URL(string: "ftp://x")!) == nil)
    }

    @Test func parsesServerMessages() {
        #expect(VoiceStream.Message(#"{"type":"ready"}"#) == .ready)
        #expect(VoiceStream.Message(#"{"type":"partial","text":"hi"}"#) == .partial("hi"))
        #expect(VoiceStream.Message(#"{"type":"error","error":"voiceQuotaExceeded"}"#) == .error("voiceQuotaExceeded"))
        #expect(VoiceStream.Message("nonsense") == .other)
    }

    /// 没有服务器在听：很快失败，告诉用户连不上。
    @Test func unreachableServerIsOffline() async {
        await #expect(throws: AppError("voiceOffline")) {
            _ = try await VoiceStream.open(server: URL(string: "http://127.0.0.1:9")!, token: "t", hints: VoiceHints())
        }
    }
}
