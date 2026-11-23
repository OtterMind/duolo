package ai.ottermind.duolo.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64

private val JsonElement.string get() = jsonPrimitive.content

class WireContractTest {
    private val contract = Repo.json("mobile/contract/wire.json")

    @Test
    fun phoneMessagesMatchTheDesktop() {
        for (c in contract.at("phone").jsonArray) {
            val message = WireJson.decodeFromJsonElement<PhoneMessage>(c.at("json"))
            assertEquals(c.at("name").string, c.at("json"), message.toJson())
        }
    }

    @Test
    fun desktopMessagesAreUnderstood() {
        val kinds = contract.at("desktop").jsonArray.map { c ->
            when (val m = DesktopMessage.parse(c.at("json"))) {
                is DesktopMessage.OverviewChanged -> {
                    assertEquals(7, m.overview.rev)
                    assertEquals(listOf("main", "develop"), m.overview.projects[0].branches)
                    assertEquals(42L, m.overview.tasks[0].prompt?.seq)
                    assertEquals(96, m.overview.tasks[0].diff?.insertions)
                    assertEquals(2, m.overview.tasks[1].review?.issues)
                    "overview"
                }
                is DesktopMessage.Events -> {
                    assertEquals((1L..11L).toList(), m.events.map { it.seq })
                    assertTrue(m.events.none { it.kind == TaskEvent.Kind.Unknown })
                    assertEquals(TaskEvent.Kind.Prompt("Proceed?", "pnpm test", listOf("Yes", "No")), m.events[6].kind)
                    assertEquals(c.at("json").at("events"), JsonArray(m.events.map { it.raw }))
                    "events"
                }
                is DesktopMessage.Review -> "review".also { assertTrue(m.output.startsWith("[ISSUES 2]")) }
                is DesktopMessage.Result -> "result".also { assertEquals(if (m.result.ok) 9L else null, m.result.task) }
                DesktopMessage.Unknown -> "unknown"
            }
        }
        assertEquals(listOf("overview", "events", "review", "result", "result"), kinds)
    }

    @Test
    fun newerDesktopsDoNotBreakOldPhones() {
        val cases = contract.at("tolerated").jsonArray
        assertEquals(DesktopMessage.Unknown, DesktopMessage.parse(cases[0].at("json")))
        val events = (DesktopMessage.parse(cases[1].at("json")) as DesktopMessage.Events).events
        assertEquals(TaskEvent.Kind.Unknown, events[0].kind)
        assertEquals(TaskEvent.Kind.Message(false, "hi"), events[1].kind)
        assertEquals("cube", events[0].raw.at("shape").string)
    }
}

class CryptoContractTest {
    private val contract = Repo.json("mobile/contract/crypto.json")
    private fun bytes(v: JsonElement) = Base64.getDecoder().decode(v.string)
    private fun channel() =
        SecureChannel(bytes(contract.at("phone").at("privateKey")), bytes(contract.at("desktop").at("publicKey")), bytes(contract.at("secret")))

    @Test
    fun matchesTheReferenceVectors() {
        val channel = channel()
        for (m in contract.at("messages").jsonArray) {
            val (id, from, to) = listOf("id", "from", "to").map { m.at(it).string }
            val plaintext = m.at("plaintext").string.toByteArray()
            if (m.at("direction").string == "phoneToDesktop") {
                assertEquals(m.at("body").string, channel.seal(plaintext, id, from, to, bytes(m.at("nonce"))))
            } else {
                assertEquals(m.at("plaintext").string, channel.open(m.at("body").string, id, from, to).decodeToString())
            }
        }
    }

    @Test
    fun rejectsAnythingChanged() {
        val channel = channel()
        val body = contract.at("messages").jsonArray[1].at("body").string
        fun fails(body: String, id: String = "m-2", from: String = "desktop-1", to: String = "phone-1") =
            runCatching { channel.open(body, id, from, to) }.exceptionOrNull() is SecureChannel.BadMessage
        assertTrue(!fails(body))
        assertTrue("换编号重放", fails(body, id = "m-9"))
        assertTrue(fails(body, from = "desktop-2") && fails(body, to = "phone-2"))
        assertTrue(fails("") && fails("AQ==") && fails("not base64"))
        for ((index, what) in listOf(-1 to "内容", 0 to "版本")) {
            val data = Base64.getDecoder().decode(body)
            val i = if (index < 0) data.lastIndex else index
            data[i] = (data[i].toInt() xor 3).toByte()
            assertTrue(what, fails(Base64.getEncoder().encodeToString(data)))
        }
    }

    @Test
    fun rejectsBadKeys() {
        val mine = DeviceKey()
        assertThrows(SecureChannel.BadKey::class.java) { SecureChannel(mine.privateKey, ByteArray(32), ByteArray(32) { 1 }) }
        assertThrows(SecureChannel.BadKey::class.java) { SecureChannel(mine.privateKey, DeviceKey().publicKey, ByteArray(8)) }
        val other = SecureChannel(bytes(contract.at("phone").at("privateKey")), bytes(contract.at("desktop").at("publicKey")), ByteArray(32) { 9 })
        val body = contract.at("messages").jsonArray[1].at("body").string
        assertThrows(SecureChannel.BadMessage::class.java) { other.open(body, "m-2", "desktop-1", "phone-1") }
    }
}

class BehaviorContractTest {
    private val contract = Repo.json("mobile/contract/behavior.json")

    /** 样例里只写关心的字段，其余补默认值。 */
    private fun task(partial: JsonElement): TaskOverview {
        val agent = buildJsonObject {
            put("kind", "claude")
            put("name", "Claude")
        }
        val defaults = mapOf(
            "id" to JsonPrimitive(1), "project" to JsonPrimitive("shop"), "title" to JsonPrimitive("t"), "writer" to agent, "reviewer" to agent,
            "updatedAt" to JsonPrimitive(0),
        )
        return WireJson.decodeFromJsonElement(JsonObject(defaults + partial.jsonObject))
    }

    @Test
    fun previews() {
        for (c in contract.at("preview").jsonArray) assertEquals(c.at("in").string, c.at("out").string, Excerpt.first(c.at("in").string))
    }

    @Test
    fun rows() {
        for (c in contract.at("rows").jsonArray) {
            val row = TaskRow(task(c.at("task")))
            val name = c.at("name").string
            assertEquals(name, c.at("state").string, row.state::class.simpleName!!.replaceFirstChar { it.lowercase() })
            assertEquals(name, c.at("group").string, row.group.name.lowercase())
            assertEquals(name, c.at("detail").string, row.detail)
        }
    }

    @Test
    fun order() {
        val tasks = contract.at("sort").at("tasks").jsonArray.map(::task)
        assertEquals(contract.at("sort").at("order").jsonArray.map { it.jsonPrimitive.long }, TaskRow.rows(tasks).map { it.id })
    }

    @Test
    fun timelines() {
        for (c in contract.at("timeline").jsonArray) {
            val t = Timeline(c.at("events").jsonArray.map { TaskEvent.parse(it.jsonObject) })
            fun show(s: Timeline.Step) = "${s.kind.name.lowercase()} ${s.target}"
            val items = t.items.map { item ->
                buildJsonObject {
                    put("seq", item.seq)
                    when (item) {
                        is Timeline.Item.User -> {
                            put("kind", "user")
                            put("text", item.text)
                        }
                        is Timeline.Item.Agent -> {
                            put("kind", "agent")
                            put("text", item.text)
                        }
                        is Timeline.Item.Steps -> {
                            val n = Timeline.Counts.of(item.steps)
                            put("kind", "steps")
                            put("read", n.read)
                            put("edit", n.edit)
                            put("run", n.run)
                            put("other", n.other)
                            put("steps", JsonArray(item.steps.map { JsonPrimitive(show(it)) }))
                        }
                        is Timeline.Item.Review -> {
                            put("kind", "review")
                            put("reviewer", item.reviewer)
                            put("ok", item.ok)
                            put("verdict", item.verdict)
                            put("issues", item.issues)
                            put("summary", item.summary)
                        }
                    }
                }
            }
            assertEquals(c.at("name").string, c.at("items"), JsonArray(items))
            assertEquals(c.at("activity").jsonPrimitive.contentOrNull, t.activity?.let(::show))
            val diff = c.at("diff").takeIf { it != JsonNull }?.jsonArray?.map { it.jsonPrimitive.int }
            assertEquals(diff, t.diff?.let { listOf(it.files, it.insertions, it.deletions) })
            assertEquals(c.at("closed").jsonPrimitive.boolean, t.closed)
        }
    }

    @Test
    fun pairingLinks() {
        val base64 = Base64.getEncoder()
        for (c in contract.at("pairing").at("valid").jsonArray) {
            val link = PairingLink.parse(c.at("link").string)!!
            assertEquals(c.at("server").string, link.server)
            assertEquals(c.at("code").string, link.code)
            assertEquals(c.at("key").string, base64.encodeToString(link.desktopKey))
            assertEquals(c.at("secret").string, base64.encodeToString(link.secret))
        }
        for (c in contract.at("pairing").at("invalid").jsonArray) assertNull(c.at("why").string, PairingLink.parse(c.at("link").string))
        assertNull(PairingLink.parse(""))
        assertNull(PairingLink.parse("not a link at all %%"))
    }

    @Test
    fun locales() {
        for (c in contract.at("locales").jsonArray) assertEquals(c.at("code").string, Localizer.match(c.at("tags").jsonArray.map { it.string }))
    }

    @Test
    fun everyLanguageHasEveryKey() {
        val dir = Repo.root.resolve("locales")
        fun keys(code: String) = WireJson.decodeFromString<Map<String, String>>(dir.resolve("$code.json").readText()).keys
        for ((code, _) in Localizer.locales) {
            val t = Localizer(code) { dir.resolve(it).inputStream() }
            assertNotEquals(code, "mobile.send", t("mobile.send"))
            assertEquals(code, keys("en"), keys(code))
        }
        val zh = Localizer("zh-CN") { dir.resolve(it).inputStream() }
        assertEquals("2 个问题", zh("mobile.state.issues", "count" to 2))
        assertEquals("no.such.key", zh("no.such.key"))
        assertTrue("weird" in zh.error("weird"))
    }

    @Test
    fun backoff() {
        val delays = contract.at("backoff").at("delays").jsonArray.map { it.jsonPrimitive.long }
        assertEquals(delays, delays.indices.map { Backoff.delay(it) })
    }
}
