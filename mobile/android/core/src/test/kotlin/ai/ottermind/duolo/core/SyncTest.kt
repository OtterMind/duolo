package ai.ottermind.duolo.core

import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import org.bouncycastle.crypto.agreement.X25519Agreement
import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.crypto.generators.HKDFBytesGenerator
import org.bouncycastle.crypto.modes.ChaCha20Poly1305
import org.bouncycastle.crypto.params.AEADParameters
import org.bouncycastle.crypto.params.HKDFParameters
import org.bouncycastle.crypto.params.KeyParameter
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.util.Base64
import java.util.UUID
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicInteger

private fun JsonObject.text(key: String) = this[key]?.jsonPrimitive?.contentOrNull

/** 假的服务器：手机发来的记下来，想让手机收到什么就 `push`。 */
private class FakeRelay : RelayTransport {
    val opens = AtomicInteger()
    private var greeted = 0

    @Volatile
    var refuse = false

    @Volatile
    private var inbox = Channel<String>(Channel.UNLIMITED)
    val sent = CopyOnWriteArrayList<JsonObject>()

    override suspend fun open(server: String, token: String): RelayConnection {
        val mine = Channel<String>(Channel.UNLIMITED)
        inbox = mine
        opens.incrementAndGet()
        if (refuse) throw RelayClosed(0)
        return object : RelayConnection {
            override fun send(text: String): Boolean {
                sent += WireJson.parseToJsonElement(text).jsonObject
                return true
            }

            override suspend fun receive() = mine.receive()
            override fun close() {
                mine.close(RelayClosed(1001))
            }
        }
    }

    /** 等手机连上（上次打过招呼之后新的一条连接）。 */
    suspend fun waitForConnection() {
        until("a new connection") { opens.get() > greeted }
        greeted = opens.get()
    }

    fun push(frame: String) {
        inbox.trySend(frame)
    }

    fun drop(code: Int) {
        inbox.close(RelayClosed(code))
    }
}

/** 手机和假电脑的一整套。 */
private class Rig(timing: SyncEngine.Timing = SyncEngine.Timing(backoff = { 5L }), val store: LocalStore = LocalStore(JdbcDb())) {
    val relay = FakeRelay()
    val engine: SyncEngine
    private val toPhone: ByteArray
    private val fromPhone: ByteArray

    init {
        val desktop = DeviceKey()
        val phone = DeviceKey()
        val secret = ByteArray(32) { it.toByte() }
        val b64 = Base64.getEncoder()
        val computer = Computer("mac", "MacBook Pro", "mac", "https://relay.test", "phone", b64.encodeToString(desktop.publicKey))
        store.add(computer, 0)
        engine = SyncEngine(
            computer, Secrets("t", b64.encodeToString(phone.privateKey), b64.encodeToString(secret)), store, relay, timing.copy(), now = { 1000 },
        )
        val shared = ByteArray(32)
        X25519Agreement().apply { init(X25519PrivateKeyParameters(desktop.privateKey, 0)) }
            .calculateAgreement(X25519PublicKeyParameters(phone.publicKey, 0), shared, 0)
        fun key(d: String) = ByteArray(32).also {
            HKDFBytesGenerator(SHA256Digest()).apply {
                init(HKDFParameters(shared, secret, "duolo/v1 $d".toByteArray() + desktop.publicKey + phone.publicKey))
                generateBytes(it, 0, 32)
            }
        }
        toPhone = key("desktop->phone")
        fromPhone = key("phone->desktop")
    }

    private fun chacha(encrypt: Boolean, key: ByteArray, nonce: ByteArray, input: ByteArray, aad: String): ByteArray {
        val c = ChaCha20Poly1305()
        c.init(encrypt, AEADParameters(KeyParameter(key), 128, nonce, aad.toByteArray()))
        val out = ByteArray(c.getOutputSize(input.size))
        c.doFinal(out, c.processBytes(input, 0, input.size, out, 0))
        return out
    }

    suspend fun hello(online: Boolean) {
        relay.waitForConnection()
        relay.push("""{"type":"hello","peers":[{"id":"mac","name":"MacBook Pro","platform":"mac","online":$online}]}""")
    }

    /** 电脑发来一条消息。 */
    fun desktop(json: String, id: String = UUID.randomUUID().toString()) {
        val nonce = ByteArray(12) { 7 }
        val sealed = chacha(true, toPhone, nonce, json.toByteArray(), "duolo/v1\n$id\nmac\nphone")
        raw(id, Base64.getEncoder().encodeToString(byteArrayOf(1) + nonce + sealed))
    }

    fun raw(id: String, body: String) = relay.push("""{"type":"message","id":"$id","from":"mac","body":"$body","at":""}""")

    /** 手机发给电脑的，解开之后的内容。 */
    val received: List<JsonObject>
        get() = relay.sent.filter { it.text("type") == "send" }.map { f ->
            val data = Base64.getDecoder().decode(f.text("body"))
            val plain = chacha(false, fromPhone, data.copyOfRange(1, 13), data.copyOfRange(13, data.size), "duolo/v1\n${f.text("id")}\nphone\nmac")
            WireJson.parseToJsonElement(plain.decodeToString()).jsonObject
        }

    val acked: List<String> get() = relay.sent.filter { it.text("type") == "ack" }.flatMap { f -> f.getValue("ids").jsonArray.map { it.jsonPrimitive.content } }

    fun asked(type: String, after: Long? = null) =
        received.any { it.text("type") == type && (after == null || it["after"]?.jsonPrimitive?.longOrNull == after) }

    suspend fun online() {
        engine.start()
        hello(true)
        until("online") { engine.link.value == SyncEngine.Link.Connected(true) }
    }
}

private fun overview(rev: Int, vararg tasks: Pair<Int, Int>) = """{"v":1,"type":"overview","rev":$rev,"tasks":[${
    tasks.joinToString(",") { (id, seq) ->
        """{"id":$id,"project":"shop","title":"T","status":"running","writer":{"kind":"claude","name":"Claude"},"reviewer":{"kind":"codex","name":"Codex"},"updatedAt":1,"seq":$seq}"""
    }
}]}"""

private fun events(range: IntRange, latest: Int) = """{"v":1,"type":"events","task":3,"events":[${
    range.joinToString(",") { """{"task":3,"seq":$it,"at":1,"type":"message","speaker":"agent","agent":0,"text":"m$it"}""" }
}],"latest":$latest}"""

private suspend fun errorOf(body: suspend () -> Unit): AppError? = try {
    body()
    null
} catch (e: AppError) {
    e
}

class SyncTest {
    private fun test(body: suspend kotlinx.coroutines.CoroutineScope.() -> Unit) = runBlocking { body() }

    @Test
    fun asksForAnOverviewWhenTheComputerIsOnline() = test {
        val rig = Rig()
        rig.online()
        until("sync") { rig.asked("sync") }
        rig.desktop(overview(1, 3 to 0), "o1")
        until("ack") { "o1" in rig.acked }
        assertEquals(listOf(3L), rig.store.overview("mac")?.tasks?.map { it.id })
        rig.engine.stop()
    }

    @Test
    fun doesNotQueueRequestsForAComputerThatIsOffline() = test {
        val rig = Rig()
        rig.engine.start()
        rig.hello(false)
        until("connected") { rig.engine.link.value == SyncEngine.Link.Connected(false) }
        delay(50)
        assertTrue(rig.received.isEmpty())
        rig.relay.push("""{"type":"presence","device":"mac","online":true}""")
        until("sync") { rig.asked("sync") }
        rig.engine.stop()
    }

    @Test
    fun olderOverviewsAreIgnored() = test {
        val rig = Rig()
        rig.online()
        rig.desktop(overview(5, 3 to 0), "new")
        rig.desktop(overview(4, 9 to 0), "old")
        until("acks") { rig.acked.containsAll(listOf("new", "old")) }
        assertEquals(listOf(3L), rig.store.overview("mac")?.tasks?.map { it.id })
        rig.engine.stop()
    }

    @Test
    fun eventsSurviveDuplicatesGapsAndPaging() = test {
        val rig = Rig()
        rig.online()
        rig.desktop(overview(1, 3 to 6), "o")
        until("overview") { "o" in rig.acked }
        rig.engine.open(3)
        until("fetch") { rig.asked("fetch", 0) }
        rig.desktop(events(1..2, 6), "a")
        until("next page") { rig.asked("fetch", 2) }
        rig.desktop(events(1..3, 6), "dup")
        rig.desktop(events(6..6, 6), "gap")
        until("refetch") { rig.asked("fetch", 3) && "gap" in rig.acked }
        assertEquals("跳号的不能存", 3L, rig.store.lastSeq("mac", 3))
        rig.desktop(events(4..6, 6), "rest")
        until("all") { rig.store.lastSeq("mac", 3) == 6L }
        assertEquals((1L..6L).toList(), rig.store.events("mac", 3).map { it.seq })
        rig.engine.stop()
    }

    @Test
    fun startsOverWhenTheComputerLostItsHistory() = test {
        val rig = Rig()
        rig.online()
        rig.engine.open(3)
        rig.desktop(events(1..9, 9))
        until("first") { rig.store.lastSeq("mac", 3) == 9L }
        rig.desktop(events(1..2, 2))
        until("reset") { rig.store.lastSeq("mac", 3) == 2L }
        rig.engine.stop()
    }

    @Test
    fun closedTasksAreForgotten() = test {
        val rig = Rig()
        rig.online()
        rig.engine.open(3)
        rig.desktop(overview(1, 3 to 2))
        rig.desktop(events(1..2, 2))
        until("events") { rig.store.lastSeq("mac", 3) == 2L }
        rig.desktop(overview(2))
        until("gone") { rig.store.lastSeq("mac", 3) == 0L }
        rig.engine.stop()
    }

    @Test
    fun badAndUnknownMessagesDoNotStopTheStream() = test {
        val rig = Rig()
        rig.online()
        rig.raw("bad", "AQAAAA==")
        rig.desktop("""{"v":1,"type":"hologram"}""", "x")
        rig.desktop("not json", "y")
        rig.desktop(overview(1, 3 to 0), "good")
        until("all acked") { rig.acked.containsAll(listOf("bad", "x", "y", "good")) }
        assertTrue(rig.store.overview("mac") != null)
        rig.engine.stop()
    }

    @Test
    fun messagesSentOfflineGoOutOnceWithTheSameId() = test {
        val rig = Rig()
        rig.engine.submit(Command.SendMessage(3, "hi"))
        val id = rig.store.outbox("mac")[0].id
        fun times() = rig.relay.sent.count { it.text("id") == id }
        rig.engine.start()
        rig.hello(false)
        until("sent") { times() == 1 }
        // 还没收到 sent 就断了：重连后用同一个编号再发。
        rig.relay.drop(1006)
        until("reconnect") { rig.relay.opens.get() == 2 }
        rig.hello(false)
        until("resent") { times() == 2 }
        rig.relay.push("""{"type":"sent","id":"$id"}""")
        until("marked") { rig.store.outbox("mac")[0].state == OutboxItem.State.sent }
        // 服务器收下了：再重连不重发，等电脑的结果。
        rig.relay.drop(1006)
        until("reconnect again") { rig.relay.opens.get() == 3 }
        rig.hello(true)
        rig.desktop("""{"v":1,"type":"result","id":"$id","result":{"ok":true}}""")
        until("done") { rig.store.outbox("mac").isEmpty() }
        assertEquals(2, times())
        rig.engine.stop()
    }

    @Test
    fun rejectedCommandsStayVisibleAndCanBeRetried() = test {
        val rig = Rig()
        rig.online()
        rig.engine.submit(Command.StartReview(3))
        val id = rig.store.outbox("mac")[0].id
        rig.desktop("""{"v":1,"type":"result","id":"$id","result":{"ok":false,"error":"taskNotFound"}}""")
        until("failed") { rig.store.outbox("mac").firstOrNull()?.state == OutboxItem.State.failed }
        assertEquals("taskNotFound", rig.store.outbox("mac")[0].error)
        rig.engine.retry(id)
        val again = rig.store.outbox("mac")
        assertTrue(again.size == 1 && again[0].id != id && again[0].state == OutboxItem.State.queued)
        rig.engine.stop()
    }

    @Test
    fun answersAreNeverQueued() = test {
        val rig = Rig()
        assertEquals(AppError.computerOffline, errorOf { rig.engine.submit(Command.Answer(3, 0, 7)) })
        assertTrue(rig.store.outbox("mac").isEmpty())
        rig.online()
        val answer = async { errorOf { rig.engine.submit(Command.Answer(3, 0, 7)) } }
        until("sent") { rig.asked("command") }
        val command = rig.received.first { it.text("type") == "command" }
        assertEquals(7L, command.getValue("command").jsonObject.getValue("prompt").jsonPrimitive.long)
        rig.desktop("""{"v":1,"type":"result","id":"${command.text("id")}","result":{"ok":false,"error":"promptChanged"}}""")
        assertEquals(AppError("promptChanged"), answer.await())
        assertTrue(rig.store.outbox("mac").isEmpty())
        rig.engine.stop()
    }

    @Test
    fun answersFailWhenTheConnectionDropsOrNobodyReplies() = test {
        val rig = Rig(SyncEngine.Timing(reply = 80, backoff = { 5L }))
        rig.online()
        assertEquals(AppError.network, errorOf { rig.engine.submit(Command.Interrupt(3)) })
        rig.engine.stop()

        val slow = Rig()
        slow.online()
        val answer = async { errorOf { slow.engine.submit(Command.Interrupt(3)) } }
        until("sent") { slow.asked("command") }
        slow.relay.drop(1006)
        assertEquals(AppError.network, answer.await())
        slow.engine.stop()
    }

    @Test
    fun stopsWhenUnlinked() = test {
        val rig = Rig()
        rig.online()
        rig.relay.drop(4401)
        until("unlinked") { rig.engine.link.value == SyncEngine.Link.Unlinked }
        delay(50)
        assertEquals("登录失效后不再重连", 1, rig.relay.opens.get())
    }

    @Test
    fun keepsReconnectingAndStops() = test {
        val rig = Rig()
        rig.relay.refuse = true
        rig.engine.start()
        until("several attempts") { rig.relay.opens.get() >= 4 }
        rig.relay.refuse = false
        rig.relay.waitForConnection()
        until("online") {
            rig.relay.push("""{"type":"hello","peers":[{"id":"mac","name":"MacBook Pro","platform":"mac","online":true}]}""")
            rig.engine.link.value == SyncEngine.Link.Connected(true)
        }
        rig.engine.stop()
        val before = rig.relay.opens.get()
        delay(60)
        assertEquals("停了就不再重连", before, rig.relay.opens.get())
        rig.engine.start()
        until("restarted") { rig.relay.opens.get() > before }
        rig.engine.stop()
    }

    @Test
    fun noticesADeadConnection() = test {
        val rig = Rig(SyncEngine.Timing(ping = 20, silence = 60, backoff = { 5L }))
        rig.online()
        until("ping") { rig.relay.sent.any { it.text("type") == "ping" } }
        until("reconnect") { rig.relay.opens.get() >= 2 }
        rig.engine.stop()
    }

    @Test
    fun dataSurvivesARestart() = test {
        val file = File.createTempFile("duolo", ".sqlite")
        val db = JdbcDb(file.path)
        val rig = Rig(store = LocalStore(db))
        rig.engine.submit(Command.SendMessage(3, "hi"))
        rig.online()
        rig.engine.open(3)
        rig.desktop(events(1..2, 2))
        until("events") { rig.store.lastSeq("mac", 3) == 2L }
        rig.engine.stop()
        db.close()
        val again = LocalStore(JdbcDb(file.path))
        assertEquals(listOf("mac"), again.computers().map { it.id })
        assertEquals(2, again.events("mac", 3).size)
        assertEquals(1, again.outbox("mac").size)
        again.remove("mac")
        assertTrue("断开后全部清掉", again.events("mac", 3).isEmpty() && again.outbox("mac").isEmpty())
        file.delete()
    }

    /** 和联调沙箱（server/dev/mobile-sandbox.ts）里真的服务端、真的加密走一遍。没设 DUOLO_SANDBOX 就跳过。 */
    @Test
    fun sandbox() {
        val sandbox = System.getenv("DUOLO_SANDBOX") ?: return
        test {
            val http = defaultHttp()
            fun control(path: String) = java.net.URI("$sandbox/$path").toURL().readText()
            val store = LocalStore(JdbcDb())
            val secrets = MemorySecrets()
            val pairing = Pairing(store, secrets, http)
            val link = PairingLink.parse(control("link"))!!
            val computer = pairing.connect(link, "Test Android", "android", 1)
            assertEquals(AppError("invalidPairingCode"), errorOf { pairing.connect(link, "Again", "android", 2) })

            val engine = SyncEngine(computer, secrets.load(computer.id)!!, store, OkHttpTransport(http))
            engine.start()
            until("overview", 10_000) { (store.overview(computer.id)?.tasks?.size ?: 0) >= 8 }
            val rows = TaskRow.rows(store.overview(computer.id)!!.tasks)
            val done = rows.first { it.state == TaskRow.State.Passed }
            engine.open(done.id)
            until("events", 10_000) { store.lastSeq(computer.id, done.id) == done.task.seq }
            until("review", 10_000) { store.review(computer.id, done.id)?.startsWith("[PASS]") == true }

            // 新建一个任务，问它话；电脑离线时排队，上线后送达。
            engine.submit(Command.CreateTask("docs", "Kotlin was here"))
            until("new task", 10_000) { store.overview(computer.id)!!.tasks.any { it.title == "Kotlin was here" } }
            control("offline")
            until("offline", 10_000) { engine.link.value == SyncEngine.Link.Connected(false) }
            assertEquals(AppError.computerOffline, errorOf { engine.submit(Command.Interrupt(done.id)) })
            val mine = store.overview(computer.id)!!.tasks.first { it.title == "Kotlin was here" }.id
            engine.submit(Command.SendMessage(mine, "多字节 👋"))
            until("relayed", 10_000) { store.outbox(computer.id).firstOrNull()?.state == OutboxItem.State.sent }
            control("online")
            until("delivered", 10_000) { store.outbox(computer.id).isEmpty() }
            engine.open(mine)
            until("message event", 10_000) { store.events(computer.id, mine).any { it.kind == TaskEvent.Kind.Message(true, "多字节 👋") } }
            assertEquals(AppError("promptChanged"), errorOf { engine.submit(Command.Answer(mine, 0, 999)) })

            ServerApi(computer.server, http).unlink(computer.deviceId, secrets.load(computer.id)!!.token)
            until("unlinked", 10_000) { engine.link.value == SyncEngine.Link.Unlinked }
            pairing.disconnect(computer)
            if (store.computers().isNotEmpty()) fail("should be gone")
        }
    }
}
