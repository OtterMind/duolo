package io.duolo.app.core

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.Base64
import java.util.UUID
import kotlin.coroutines.coroutineContext

/**
 * 和一台电脑的同步。一台电脑一个，全部状态只在自己的单线程上改。和 iOS 的 SyncEngine 行为一致。
 *
 * 稳定性靠这几条（详见 docs/mobile.md）：
 * - 收到的消息先存进数据库再确认，没存成就不确认，服务器下次补发；
 * - 重复、乱序、缺了中间的都能处理：总览看版本号，事件看编号，缺了就重新要；
 * - 命令先存再发，同一个编号重发不会执行两次；回答和打断只在电脑在线时发，不排队；
 * - 断线按退避重连，半死的连接靠心跳发现。
 */
class SyncEngine(
    val computer: Computer,
    secrets: Secrets,
    private val store: LocalStore,
    private val transport: RelayTransport,
    private val timing: Timing = Timing(),
    dispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val now: () -> Long = System::currentTimeMillis,
) {
    sealed interface Link {
        data object Connecting : Link

        /** 连上服务器了；电脑不一定在线。 */
        data class Connected(val computerOnline: Boolean) : Link

        /** 断了，等一会儿重连。 */
        data object Waiting : Link

        /** 这台手机被解除了，要重新配对。 */
        data object Unlinked : Link
    }

    sealed interface Change {
        data object Overview : Change
        data class Task(val id: Long) : Change
        data object Outbox : Change
        data class Failure(val error: AppError) : Change
    }

    data class Timing(
        val ping: Long = 25_000,
        val silence: Long = 60_000,
        val reply: Long = 15_000,
        val backoff: (Int) -> Long = Backoff::jittered,
    )

    private val confined = dispatcher.limitedParallelism(1)
    private val scope = CoroutineScope(SupervisorJob() + confined)
    private val token = secrets.token
    private val channel = SecureChannel(secrets.privateKeyBytes(), Base64.getDecoder().decode(computer.desktopKey), secrets.secretBytes())

    private val linkState = MutableStateFlow<Link>(Link.Connecting)
    val link: StateFlow<Link> get() = linkState
    private val changeFlow = MutableSharedFlow<Change>(extraBufferCapacity = 256, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val changes: SharedFlow<Change> get() = changeFlow

    private var connection: RelayConnection? = null
    private var lastHeard = 0L
    private var attempt = 0
    private var runner: Job? = null
    private var wake = CompletableDeferred<Unit>()

    /** 打开着的任务，电脑上线后接着补它的事件。 */
    private var focus: Long? = null

    /**
     * 第一次打开、还没有事件的任务：要的是从哪条之后（见 [fetch]）。回来之前电脑推来的新事件不存，
     * 否则手机以为已经有到最新的了，随后回来的前面的事件会被当成旧的丢掉。
     */
    private val opening = mutableMapOf<Long, Long>()

    /** 在等结果的实时命令（回答、打断）。 */
    private val live = mutableMapOf<String, CompletableDeferred<Unit>>()

    private fun emit(change: Change) {
        changeFlow.tryEmit(change)
    }

    // ---- 生命周期 ----

    suspend fun start() = withContext(confined) {
        if (runner == null && linkState.value != Link.Unlinked) runner = scope.launch { loop() }
    }

    /** 应用进后台、换了电脑：断开，不再重连。 */
    suspend fun stop() = withContext(confined) {
        connection?.close()
        runner?.cancelAndJoin()
        runner = null
        connection = null
        failLive(AppError.network)
    }

    /** 网络恢复、回到前台：不等退避，马上重连。 */
    suspend fun nudge() = withContext(confined) {
        attempt = 0
        wake.complete(Unit)
        Unit
    }

    private suspend fun loop() {
        while (coroutineContext.isActive) {
            linkState.value = Link.Connecting
            val closed = connectOnce()
            connection?.close()
            connection = null
            failLive(AppError.network)
            if (closed == 4401) {
                linkState.value = Link.Unlinked
                runner = null
                return
            }
            linkState.value = Link.Waiting
            wake = CompletableDeferred()
            withTimeoutOrNull(timing.backoff(attempt)) { wake.await() }
            attempt += 1
        }
    }

    /** 连一次，直到断开。返回关闭码。 */
    private suspend fun connectOnce(): Int = try {
        coroutineScope {
            val connection = transport.open(computer.server, token)
            this@SyncEngine.connection = connection
            lastHeard = System.nanoTime()
            val heartbeat = launch {
                while (isActive) {
                    delay(timing.ping)
                    // 太久没有任何消息，说明连接已经断了但系统还没发现。
                    if ((System.nanoTime() - lastHeard) / 1_000_000 > timing.silence) connection.close() else connection.send(RelayFrames.ping())
                }
            }
            try {
                while (true) {
                    val text = connection.receive()
                    lastHeard = System.nanoTime()
                    handle(text)
                }
                @Suppress("UNREACHABLE_CODE")
                0
            } finally {
                heartbeat.cancel()
            }
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: RelayClosed) {
        e.code
    } catch (_: Exception) {
        0
    }

    // ---- 收 ----

    private suspend fun handle(text: String) {
        val m = runCatching { WireJson.parseToJsonElement(text).jsonObject }.getOrNull() ?: return
        fun field(key: String) = m[key]?.jsonPrimitive?.contentOrNull
        when (field("type")) {
            "hello" -> {
                attempt = 0
                val peers = runCatching { WireJson.decodeFromString<List<PeerInfo>>(m.getValue("peers").jsonArray.toString()) }.getOrDefault(emptyList())
                val peer = peers.firstOrNull { it.id == computer.id }
                if (peer != null && peer.name != computer.name) runCatching { store.rename(computer.id, peer.name) }
                linkState.value = Link.Connected(peer?.online ?: false)
                flushOutbox()
                if (peer?.online == true) catchUp()
            }
            "presence" -> {
                if (field("device") != computer.id) return
                val online = m["online"]?.jsonPrimitive?.boolean ?: return
                linkState.value = Link.Connected(online)
                if (online) catchUp()
            }
            "message" -> onMessage(field("id") ?: return, field("from"), field("body").orEmpty())
            "sent" -> {
                val id = field("id") ?: return
                if (store.outbox(computer.id).any { it.id == id && it.state == OutboxItem.State.queued }) {
                    store.mark(id, OutboxItem.State.sent)
                    emit(Change.Outbox)
                }
            }
            "error" -> {
                val id = field("id") ?: return
                val error = if (field("code") == "unknownDevice") AppError.unauthorized else AppError.server
                live.remove(id)?.completeExceptionally(error)
                store.mark(id, OutboxItem.State.failed, error.code)
                emit(Change.Outbox)
            }
        }
    }

    private suspend fun onMessage(id: String, from: String?, body: String) {
        // 同一账号的其他设备发来的不归这里管，但要确认掉，不然会一直补发。
        val plaintext = if (from == computer.id) runCatching { channel.open(body, id, from, computer.deviceId) }.getOrNull() else null
        if (plaintext != null) {
            try {
                apply(plaintext)
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                // 没存成（例如手机满了）：不确认，以后补发。
                emit(Change.Failure(AppError.storage))
                return
            }
        }
        connection?.send(RelayFrames.ack(id))
    }

    private fun apply(plaintext: ByteArray) {
        // 读不懂的（更新的格式）当作没有，不能卡住后面的消息。
        val message = runCatching { DesktopMessage.parse(WireJson.parseToJsonElement(plaintext.decodeToString())) }.getOrNull() ?: return
        when (message) {
            is DesktopMessage.OverviewChanged -> if (store.save(message.overview, computer.id)) emit(Change.Overview)
            is DesktopMessage.Events -> {
                // 没打开过的任务不存事件，列表用总览就够了。
                val task = message.task
                val known = store.lastSeq(computer.id, task)
                if (task != focus && known == 0L) return
                if (known == 0L) {
                    // 还没有这个任务的事件：只认刚才要的那一段（从 after 的下一条开始），推来的新事件等它回来再补。
                    val after = opening[task] ?: return
                    if (message.events.minOfOrNull { it.seq } != after + 1) return
                    opening.remove(task)
                }
                when (val result = store.append(message.events, task, message.latest, computer.id)) {
                    is LocalStore.Append.Applied -> {
                        emit(Change.Task(task))
                        if (result.more && message.events.isNotEmpty()) fetch(task)
                    }
                    is LocalStore.Append.Gap -> {
                        emit(Change.Task(task))
                        request(PhoneMessage.Fetch(task, result.after))
                    }
                }
            }
            is DesktopMessage.Review -> {
                store.saveReview(message.output, message.finishedAt, message.task, computer.id)
                emit(Change.Task(message.task))
            }
            is DesktopMessage.Result -> {
                if (message.result.ok) {
                    store.dequeue(message.id)
                    live.remove(message.id)?.complete(Unit)
                } else {
                    val error = AppError(message.result.error ?: "internal")
                    store.mark(message.id, OutboxItem.State.failed, error.code)
                    live.remove(message.id)?.completeExceptionally(error)
                }
                emit(Change.Outbox)
            }
            DesktopMessage.Unknown -> Unit
        }
    }

    // ---- 要 ----

    /** 电脑在线了：要最新的总览，接着补打开着的任务。 */
    private fun catchUp() {
        request(PhoneMessage.Sync)
        focus?.let(::fetch)
    }

    private fun fetch(task: Long) {
        val last = store.lastSeq(computer.id, task)
        // 第一次打开：只要最近的一段，很长的任务不从头拉。
        val after = if (last > 0) last else store.overview(computer.id)?.tasks?.firstOrNull { it.id == task }?.let { maxOf(0, it.seq - 400) } ?: 0
        if (last == 0L) opening[task] = after
        request(PhoneMessage.Fetch(task, after))
    }

    /** 打开、离开任务。 */
    suspend fun open(task: Long?) = withContext(confined) {
        focus = task
        if (task != null && linkState.value == Link.Connected(true)) fetch(task)
    }

    /** 下拉刷新。 */
    suspend fun refresh() = withContext(confined) { if (linkState.value == Link.Connected(true)) catchUp() else nudge() }

    // ---- 发 ----

    /** 让电脑做一件事。回答和打断等电脑的结果（不成抛错误）；其他的先存下来，电脑离线也会送到。 */
    suspend fun submit(command: Command) = withContext(confined) {
        val item = OutboxItem(UUID.randomUUID().toString(), command, now(), OutboxItem.State.queued)
        if (command.isLive) {
            if (linkState.value != Link.Connected(true)) throw AppError.computerOffline
            val reply = CompletableDeferred<Unit>()
            live[item.id] = reply
            if (!transmit(item)) reply.completeExceptionally(AppError.network)
            try {
                withTimeoutOrNull(timing.reply) { reply.await() } ?: throw AppError.network
            } finally {
                live.remove(item.id)
            }
        } else {
            try {
                store.enqueue(item, computer.id)
            } catch (_: Exception) {
                throw AppError.storage
            }
            emit(Change.Outbox)
            if (linkState.value is Link.Connected) transmit(item)
        }
        Unit
    }

    /** 没送到的再发一次（用新编号：旧编号电脑可能已经处理过并拒绝了）。 */
    suspend fun retry(id: String) = withContext(confined) {
        val item = store.outbox(computer.id).firstOrNull { it.id == id && it.state == OutboxItem.State.failed } ?: return@withContext
        store.dequeue(id)
        submit(item.command)
    }

    suspend fun discard(id: String) = withContext(confined) {
        store.dequeue(id)
        emit(Change.Outbox)
    }

    /** 重新连上：服务器还没收下的全部再发一遍，编号不变，重复的服务器会去掉。 */
    private fun flushOutbox() {
        store.outbox(computer.id).filter { it.state == OutboxItem.State.queued }.forEach(::transmit)
    }

    private fun transmit(item: OutboxItem): Boolean = send(PhoneMessage.Run(item.id, item.at, item.command), item.id)

    private fun request(message: PhoneMessage) {
        send(message, UUID.randomUUID().toString())
    }

    private fun send(message: PhoneMessage, id: String): Boolean {
        val connection = connection ?: return false
        val json: JsonObject = message.toJson()
        val body = channel.seal(json.toString().toByteArray(), id, computer.deviceId, computer.id)
        val sent = connection.send(RelayFrames.send(computer.id, id, body))
        // 发不出去就是断了，接收那边会发现并重连。
        if (!sent) connection.close()
        return sent
    }

    private fun failLive(error: AppError) {
        val waiting = live.values.toList()
        live.clear()
        waiting.forEach { it.completeExceptionally(error) }
    }
}

/** 配对和解除。 */
class Pairing(private val store: LocalStore, private val secrets: SecretStore, private val http: okhttp3.OkHttpClient) {
    /** 用二维码里的内容连接电脑。 */
    suspend fun connect(link: PairingLink, deviceName: String, platform: String, now: Long): Computer {
        val key = DeviceKey()
        val api = ServerApi(link.server, http)
        val claim = api.claim(link.code, deviceName, platform, key.publicKey)
        val base64 = Base64.getEncoder()
        // 公钥以二维码里的为准：服务器给的不一样，说明它换了一把。
        if (claim.desktop.publicKey != base64.encodeToString(link.desktopKey)) {
            runCatching { api.unlink(claim.device.id, claim.token) }
            throw AppError.badPairingLink
        }
        val computer = Computer(
            claim.desktop.id, claim.desktop.name, claim.desktop.platform, link.server, claim.device.id, base64.encodeToString(link.desktopKey),
        )
        try {
            // 先存机密：只有电脑没有机密的话，界面上会有一台连不上的电脑。
            secrets.save(Secrets(claim.token, base64.encodeToString(key.privateKey), base64.encodeToString(link.secret)), computer.id)
            store.add(computer, now)
        } catch (_: Exception) {
            throw AppError.storage
        }
        return computer
    }

    /** 断开一台电脑。服务器连不上也照样清掉手机上的。 */
    suspend fun disconnect(computer: Computer) {
        runCatching { secrets.load(computer.id) }.getOrNull()?.let { saved ->
            runCatching { ServerApi(computer.server, http).unlink(computer.deviceId, saved.token) }
        }
        try {
            store.remove(computer.id)
            secrets.remove(computer.id)
        } catch (_: Exception) {
            throw AppError.storage
        }
    }
}
