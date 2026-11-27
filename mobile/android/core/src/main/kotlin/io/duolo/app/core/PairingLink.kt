package io.duolo.app.core

import java.net.URI
import java.net.URLDecoder
import java.util.Base64

/** 电脑上二维码的内容：服务器地址、一次性配对码、电脑的公钥、一次性密码。 */
class PairingLink private constructor(val server: String, val code: String, val desktopKey: ByteArray, val secret: ByteArray) {
    /** 显示给用户确认的服务器名字。 */
    val serverName: String get() = URI(server).authority

    companion object {
        fun parse(text: String): PairingLink? = runCatching {
            val uri = URI(text.trim())
            if (!uri.scheme.equals("duolo", ignoreCase = true) || uri.host != "pair") return null
            val query = uri.rawQuery.orEmpty().split("&").mapNotNull {
                val (key, value) = it.split("=", limit = 2).takeIf { p -> p.size == 2 } ?: return@mapNotNull null
                key to URLDecoder.decode(value, "UTF-8")
            }.toMap()
            if (query["v"] != "1") return null
            val server = serverUrl(query["server"] ?: return null) ?: return null
            val code = query["code"]?.uppercase()?.takeIf { it.length in 4..32 } ?: return null
            val decoder = Base64.getUrlDecoder()
            val key = decoder.decode(query["key"] ?: return null).takeIf { it.size == 32 } ?: return null
            val secret = decoder.decode(query["secret"] ?: return null).takeIf { it.size == 32 } ?: return null
            PairingLink(server, code, key, secret)
        }.getOrNull()

        /** 只要 https；http 只许连自己电脑或局域网里的（开发、自己架设）。 */
        internal fun serverUrl(text: String): String? {
            val uri = URI(text)
            val scheme = uri.scheme?.lowercase() ?: return null
            val host = uri.host?.takeIf { it.isNotEmpty() } ?: return null
            if (scheme != "https" && !(scheme == "http" && isLocal(host))) return null
            val port = if (uri.port >= 0) ":${uri.port}" else ""
            return "$scheme://$host$port${uri.rawPath.orEmpty().trimEnd('/')}"
        }

        internal fun isLocal(host: String): Boolean {
            if (host == "localhost" || host.endsWith(".local")) return true
            val bytes = host.split(".").map { it.toIntOrNull()?.takeIf { n -> n in 0..255 } ?: return false }
            if (bytes.size != 4) return false
            val (a, b) = bytes
            return a == 127 || a == 10 || (a == 192 && b == 168) || (a == 172 && b in 16..31)
        }
    }
}
