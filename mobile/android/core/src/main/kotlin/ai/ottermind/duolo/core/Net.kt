package ai.ottermind.duolo.core

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.io.IOException
import java.util.Base64
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.random.Random

/** 给用户看的错误：错误码，文案是 `error.<code>`。 */
data class AppError(val code: String) : Exception(code) {
    companion object {
        val network = AppError("network")
        val server = AppError("server")
        val unauthorized = AppError("unauthorized")
        val computerOffline = AppError("computerOffline")
        val storage = AppError("storage")
        val badPairingLink = AppError("badPairingLink")
    }
}

@Serializable
data class PeerInfo(val id: String, val name: String, val platform: String, val online: Boolean = false, val publicKey: String = "")

fun defaultHttp(): OkHttpClient = OkHttpClient.Builder().connectTimeout(20, TimeUnit.SECONDS).callTimeout(0, TimeUnit.SECONDS).build()

/** 服务端的 HTTP 接口（server/src/app.ts）。 */
class ServerApi(private val server: String, http: OkHttpClient) {
    private val http = http.newBuilder().callTimeout(20, TimeUnit.SECONDS).build()

    @Serializable
    data class Claim(val token: String, val device: PeerInfo, val desktop: PeerInfo)

    @Serializable
    private data class DeviceBody(val name: String, val platform: String, val publicKey: String)

    @Serializable
    private data class ClaimBody(val code: String, val device: DeviceBody)

    /** 用配对码登录，登记这台手机。 */
    suspend fun claim(code: String, name: String, platform: String, publicKey: ByteArray): Claim {
        val body = ClaimBody(code, DeviceBody(name.take(80), platform, Base64.getEncoder().encodeToString(publicKey)))
        return call("POST", "/v1/pairings/claim", Claim.serializer(), WireJson.encodeToString(body).toRequestBody("application/json".toMediaType()))
    }

    /** 解除这台手机：它的登录一起失效。 */
    suspend fun unlink(device: String, token: String) {
        call("DELETE", "/v1/devices/$device", JsonObject.serializer(), token = token)
    }

    private suspend fun <T> call(method: String, path: String, type: KSerializer<T>, body: RequestBody? = null, token: String? = null): T {
        val request = Request.Builder().url(server + path).method(method, body)
        if (token != null) request.header("authorization", "Bearer $token")
        val response = try {
            http.newCall(request.build()).await()
        } catch (_: IOException) {
            throw AppError.network
        }
        response.use {
            val text = try {
                it.body?.string().orEmpty()
            } catch (_: IOException) {
                throw AppError.network
            }
            if (!it.isSuccessful) {
                val code = runCatching { (WireJson.decodeFromString<JsonObject>(text)["error"] as? JsonPrimitive)?.contentOrNull }.getOrNull()
                throw when {
                    it.code == 401 -> AppError.unauthorized
                    code == "invalidPairingCode" -> AppError(code)
                    else -> AppError.server
                }
            }
            return runCatching { WireJson.decodeFromString(type, text) }.getOrElse { throw AppError.server }
        }
    }
}

private suspend fun Call.await(): Response = suspendCancellableCoroutine { c ->
    c.invokeOnCancellation { cancel() }
    enqueue(object : Callback {
        override fun onFailure(call: Call, e: IOException) = c.resumeWithException(e)
        override fun onResponse(call: Call, response: Response) = c.resume(response) { _, r, _ -> r.close() }
    })
}

/** 到服务器的一条连接。测试里换成假的。 */
interface RelayConnection {
    /** 发出去了（进了发送队列）返回 true。 */
    fun send(text: String): Boolean

    /** 下一条消息；断开时抛 `RelayClosed`。 */
    suspend fun receive(): String
    fun close()
}

interface RelayTransport {
    suspend fun open(server: String, token: String): RelayConnection
}

/** WebSocket 关闭码：4401 = 登录失效，4000 = 同一台设备在别处连上了。 */
class RelayClosed(val code: Int) : Exception("closed $code")

/** 第 n 次重连前等多久（毫秒）：翻倍，最多 30 秒。实际再乘 0.5～1，免得服务器恢复时一起涌上来。 */
object Backoff {
    fun delay(attempt: Int): Long = minOf(30_000L, 1000L shl attempt.coerceIn(0, 5))
    fun jittered(attempt: Int): Long = (delay(attempt) * Random.nextDouble(0.5, 1.0)).toLong()
}

class OkHttpTransport(private val http: OkHttpClient) : RelayTransport {
    override suspend fun open(server: String, token: String): RelayConnection {
        // 令牌放请求头，不放地址里，免得进日志。
        val request = Request.Builder().url("$server/v1/connect").header("authorization", "Bearer $token").build()
        return Connection(http, request)
    }

    private class Connection(http: OkHttpClient, request: Request) : WebSocketListener(), RelayConnection {
        private val inbox = Channel<String>(Channel.UNLIMITED)
        private val socket = http.newWebSocket(request, this)

        override fun onMessage(webSocket: WebSocket, text: String) {
            inbox.trySend(text)
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            inbox.close(RelayClosed(code))
            webSocket.close(1000, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            inbox.close(RelayClosed(code))
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            inbox.close(RelayClosed(0))
        }

        override fun send(text: String) = socket.send(text)
        override suspend fun receive(): String = inbox.receive()
        override fun close() {
            inbox.close(RelayClosed(1001))
            if (!socket.close(1001, null)) socket.cancel()
        }
    }
}

/** 服务器转发协议里，手机发给服务器的（server/src/relay.ts）。 */
internal object RelayFrames {
    private fun frame(type: String, body: JsonObjectBuilder.() -> Unit = {}) = buildJsonObject {
        put("type", type)
        body()
    }.toString()

    fun send(to: String, id: String, body: String) = frame("send") {
        put("to", to)
        put("id", id)
        put("body", body)
    }

    fun ack(id: String) = frame("ack") { put("ids", kotlinx.serialization.json.JsonArray(listOf(JsonPrimitive(id)))) }
    fun ping() = frame("ping")
}
