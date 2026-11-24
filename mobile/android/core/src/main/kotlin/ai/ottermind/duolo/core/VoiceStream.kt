package ai.ottermind.duolo.core

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString.Companion.toByteString

/**
 * 一次语音识别：16 kHz、16 位、单声道 PCM 通过 WebSocket 发给服务器的 `/v1/voice`，收回识别中和最终的文字。
 * 协议见 server/src/voice.ts，和电脑端（crates/duolo-voice/src/cloud.rs）、iOS 的 VoiceStream 一样。
 */
class VoiceStream private constructor(private val socket: WebSocket, val events: ReceiveChannel<Event>) {
    sealed interface Event {
        /** 整段到目前为止的文字。 */
        data class Partial(val text: String) : Event

        /** 最终结果，之后不会再有。 */
        data class Final(val text: String) : Event
        data class Failed(val error: AppError) : Event
    }

    /** 一块声音（建议 100 毫秒左右）。 */
    fun send(pcm: ByteArray, size: Int = pcm.size) {
        socket.send(pcm.toByteString(0, size))
    }

    /** 说完了：之后会收到 [Event.Final]。 */
    fun stop() {
        socket.send("""{"type":"stop"}""")
    }

    fun cancel() {
        socket.cancel()
    }

    companion object {
        /** 连服务器、等它说"可以"最长多久。 */
        private const val CONNECT_TIMEOUT_MS = 3_000L

        /** 服务器用这个关闭码表示登录凭据无效。 */
        private const val CLOSE_UNAUTHORIZED = 4401

        /** 连上服务器并等它说"可以"。服务器不能用时抛出 `voiceUnavailable`、`voiceQuotaExceeded`、`voiceOffline` 或 `unauthorized`。 */
        suspend fun open(server: String, token: String, hints: VoiceHints, http: OkHttpClient): VoiceStream {
            val events = Channel<Event>(Channel.UNLIMITED)
            val ready = CompletableDeferred<Unit>()
            val request = try {
                Request.Builder().url(server.trimEnd('/') + "/v1/voice").header("authorization", "Bearer $token").build()
            } catch (_: IllegalArgumentException) {
                throw AppError("voiceOffline")
            }
            val socket = http.newWebSocket(request, object : WebSocketListener() {
                override fun onMessage(webSocket: WebSocket, text: String) {
                    when (val message = Message.parse(text)) {
                        Message.Ready -> ready.complete(Unit)
                        is Message.Error -> if (!ready.completeExceptionally(AppError(message.code))) {
                            events.trySend(Event.Failed(AppError(message.code)))
                            events.close()
                        }
                        is Message.Partial -> events.trySend(Event.Partial(message.text))
                        is Message.Final -> {
                            events.trySend(Event.Final(message.text))
                            events.close()
                        }
                        Message.Other -> {}
                    }
                }

                override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                    ready.completeExceptionally(if (code == CLOSE_UNAUTHORIZED) AppError.unauthorized else AppError("voiceOffline"))
                    events.trySend(Event.Failed(AppError("voiceFailed")))
                    events.close()
                }

                override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                    ready.completeExceptionally(AppError("voiceOffline"))
                    events.trySend(Event.Failed(AppError("voiceFailed")))
                    events.close()
                }
            })
            val start = buildJsonObject {
                put("type", JsonPrimitive("start"))
                put("hotwords", JsonArray(hints.hotwords.map(::JsonPrimitive)))
                put("context", JsonArray(hints.context.map(::JsonPrimitive)))
            }
            socket.send(start.toString())
            try {
                withTimeout(CONNECT_TIMEOUT_MS) { ready.await() }
            } catch (e: AppError) {
                socket.cancel()
                throw e
            } catch (_: TimeoutCancellationException) {
                socket.cancel()
                throw AppError("voiceOffline")
            }
            return VoiceStream(socket, events)
        }
    }

    internal sealed interface Message {
        data object Ready : Message
        data class Partial(val text: String) : Message
        data class Final(val text: String) : Message
        data class Error(val code: String) : Message
        data object Other : Message

        companion object {
            fun parse(text: String): Message {
                val o = runCatching { Json.parseToJsonElement(text).jsonObject }.getOrNull() ?: return Other
                fun field(name: String) = (o[name] as? JsonPrimitive)?.contentOrNull
                return when (field("type")) {
                    "ready" -> Ready
                    "partial" -> Partial(field("text").orEmpty())
                    "final" -> Final(field("text").orEmpty())
                    "error" -> Error(field("error") ?: "voiceFailed")
                    else -> Other
                }
            }
        }
    }
}
