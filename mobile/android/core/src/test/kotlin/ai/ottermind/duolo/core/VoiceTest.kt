package ai.ottermind.duolo.core

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
            event("""{"task":1,"seq":3,"at":0,"type":"message","speaker":"user","agent":1,"text":"export orders"}"""),
            event("""{"task":1,"seq":4,"at":0,"type":"message","speaker":"agent","agent":1,"text":"I added `exportCsv` to `app/src/orders.ts`. Should I also handle `a b`?"}"""),
            event("""{"task":1,"seq":5,"at":0,"type":"prompt","agent":1,"question":"Run tests?","detail":"","options":["Yes","No"]}"""),
        )
        val hints = VoiceHints.of("shop", "Export orders", "export-csv", events)
        assertEquals(listOf("exportCsv", "app/src/orders.ts", "old.rs", "export-csv", "shop"), hints.hotwords)
        assertEquals("Run tests? (Yes / No)", hints.context.first())
        assertTrue(hints.context[1].startsWith("I added"))
        assertEquals("export orders", hints.context[2])
        assertEquals("Export orders", hints.context.last())
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
