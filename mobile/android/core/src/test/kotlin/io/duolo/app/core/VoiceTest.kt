package io.duolo.app.core

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** 语音：热词和对话提示（和电脑端 hints.rs、iOS 的 VoiceTests 同一组样例）、识别接口的消息。 */
class VoiceTest {
    private fun event(json: String) = TaskEvent.parse(Json.parseToJsonElement(json).jsonObject)

    @Test
    fun filesAndCodeNamesNewestFirst() {
        val events = listOf(
            event("""{"task":1,"seq":1,"at":0,"type":"tool","agent":1,"tool":"Read","target":"src/old.rs"}"""),
            event("""{"task":1,"seq":2,"at":0,"type":"tool","agent":1,"tool":"Bash","target":"cargo test --workspace"}"""),
            event("""{"task":1,"seq":3,"at":0,"type":"message","speaker":"user","agent":1,"text":"export orders from OrderList"}"""),
            event("""{"task":1,"seq":4,"at":0,"type":"message","speaker":"agent","agent":1,"text":"I added `exportCsv` to `app/src/orders.ts`. Should I also handle `a b`?"}"""),
            event("""{"task":1,"seq":5,"at":0,"type":"prompt","agent":1,"question":"Run tests?","detail":"","options":["Yes","No"]}"""),
        )
        val hints = VoiceHints.of("shop", "Export orders", "export-csv", events)
        assertEquals(listOf("exportCsv", "app/src/orders.ts", "OrderList", "old.rs", "export-csv", "shop"), hints.hotwords)
        assertEquals("Run tests? (Yes / No)", hints.context.first())
        assertTrue(hints.context[1].startsWith("I added"))
        assertEquals("export orders from OrderList", hints.context[2])
        assertEquals("Export orders", hints.context.last())
    }

    @Test
    fun wordsFromTheComputerComeFirst() {
        val hints = VoiceHints.of("shop", null, null, emptyList(), VoiceOverview(true, listOf("Claude Code", " Tauri ", "", "Tauri")))
        assertEquals(listOf("Claude Code", "Tauri", "shop"), hints.hotwords)
        assertTrue(hints.organize)
    }

    @Test
    fun codeNamesInPlainText() {
        assertEquals(
            listOf("exportCsv", "orders.ts", "duolo_core::t"),
            VoiceHints.codeNames("Fix exportCsv in orders.ts (see duolo_core::t, https://a.com/x). Then run it."),
        )
    }

    @Test
    fun joinsNamesSaidInPieces() {
        val hints = VoiceHints(listOf("useState", "exportCsv", "orders.ts", "Tauri"))
        assertEquals("加一个 useState 的 hook", hints.fixNames("加一个 use state 的 hook"))
        assertEquals("把 exportCsv 改到 orders.ts 里", hints.fixNames("把 Export CSV 改到 orders ts 里"))
        assertEquals("useState and use_state_x", hints.fixNames("usestate and use_state_x"))
        // 只说了一段、已经对了、单个词的，不动。
        assertEquals("use it, useState, tauri", hints.fixNames("use it, useState, tauri"))
    }

    @Test
    fun parsesServerMessages() {
        assertEquals(VoiceStream.Message.Ready, VoiceStream.Message.parse("""{"type":"ready"}"""))
        assertEquals(VoiceStream.Message.Partial("hi"), VoiceStream.Message.parse("""{"type":"partial","text":"hi"}"""))
        assertEquals(VoiceStream.Message.Error("voiceQuotaExceeded"), VoiceStream.Message.parse("""{"type":"error","error":"voiceQuotaExceeded"}"""))
        assertEquals(VoiceStream.Message.Other, VoiceStream.Message.parse("nonsense"))
    }

    /** 没有服务器在听：很快失败，告诉用户连不上。 */
    @Test
    fun unreachableServerIsOffline() = runBlocking {
        val error = runCatching { VoiceStream.open("http://127.0.0.1:9", "t", VoiceHints(), defaultHttp()) }.exceptionOrNull()
        assertEquals(AppError("voiceOffline"), error)
    }
}
