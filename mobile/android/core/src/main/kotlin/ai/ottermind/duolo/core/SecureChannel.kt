package ai.ottermind.duolo.core

import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.crypto.generators.HKDFBytesGenerator
import org.bouncycastle.crypto.modes.ChaCha20Poly1305
import org.bouncycastle.crypto.params.AEADParameters
import org.bouncycastle.crypto.params.HKDFParameters
import org.bouncycastle.crypto.params.KeyParameter
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import java.security.SecureRandom
import java.util.Base64

/**
 * 和一台电脑之间的端到端加密，算法见 docs/mobile.md。
 *
 * 密钥由两边的 X25519 密钥和二维码里的一次性密码一起算出。密码不经过服务器，
 * 所以服务器即使换掉公钥也算不出密钥，既看不到内容也冒充不了任何一边。
 * 用 BouncyCastle 是因为系统自带的 X25519 要 Android 13 才有。
 */
class SecureChannel(privateKey: ByteArray, desktopKey: ByteArray, secret: ByteArray) {
    class BadKey : Exception()
    class BadMessage : Exception()

    private val sendKey: ByteArray
    private val receiveKey: ByteArray
    private val random = SecureRandom()

    init {
        if (privateKey.size != 32 || desktopKey.size != 32 || secret.size != 32) throw BadKey()
        val mine = X25519PrivateKeyParameters(privateKey, 0)
        val shared = ByteArray(32)
        // 小阶点会算出全 0 的共享密钥，等于没有加密；这里会抛出来。
        try {
            mine.generateSecret(X25519PublicKeyParameters(desktopKey, 0), shared, 0)
        } catch (_: IllegalStateException) {
            throw BadKey()
        }
        val publicKey = mine.generatePublicKey().encoded
        fun key(direction: String) = ByteArray(32).also { out ->
            HKDFBytesGenerator(SHA256Digest()).apply {
                init(HKDFParameters(shared, secret, "duolo/v1 $direction".toByteArray() + desktopKey + publicKey))
                generateBytes(out, 0, out.size)
            }
        }
        sendKey = key("phone->desktop")
        receiveKey = key("desktop->phone")
    }

    /** 加密后的 base64：版本(1) + nonce(12) + 密文 + 校验(16)。 */
    fun seal(plaintext: ByteArray, id: String, from: String, to: String): String =
        seal(plaintext, id, from, to, ByteArray(12).also(random::nextBytes))

    internal fun seal(plaintext: ByteArray, id: String, from: String, to: String, nonce: ByteArray): String =
        Base64.getEncoder().encodeToString(byteArrayOf(1) + nonce + run(true, sendKey, nonce, plaintext, header(id, from, to)))

    fun open(body: String, id: String, from: String, to: String): ByteArray = try {
        val data = Base64.getDecoder().decode(body)
        if (data.size < 29 || data[0] != 1.toByte()) throw BadMessage()
        run(false, receiveKey, data.copyOfRange(1, 13), data.copyOfRange(13, data.size), header(id, from, to))
    } catch (_: Exception) {
        throw BadMessage()
    }

    private fun run(encrypt: Boolean, key: ByteArray, nonce: ByteArray, input: ByteArray, aad: ByteArray): ByteArray {
        val cipher = ChaCha20Poly1305()
        cipher.init(encrypt, AEADParameters(KeyParameter(key), 128, nonce, aad))
        val out = ByteArray(cipher.getOutputSize(input.size))
        var n = cipher.processBytes(input, 0, input.size, out, 0)
        n += cipher.doFinal(out, n)
        return out.copyOf(n)
    }

    /** 编号和收发双方一起校验：服务器不能把一条消息改个编号或换个对象再送一次。 */
    private fun header(id: String, from: String, to: String) = "duolo/v1\n$id\n$from\n$to".toByteArray()
}

/** 这台手机的一对密钥，每次配对新生成。 */
class DeviceKey {
    private val key = X25519PrivateKeyParameters(SecureRandom())
    val privateKey: ByteArray = key.encoded
    val publicKey: ByteArray = key.generatePublicKey().encoded
}
