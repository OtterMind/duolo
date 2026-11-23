package ai.ottermind.duolo

import ai.ottermind.duolo.core.Db
import ai.ottermind.duolo.core.SecretStore
import ai.ottermind.duolo.core.Secrets
import ai.ottermind.duolo.core.WireJson
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import kotlinx.serialization.encodeToString
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** 系统自带的 SQLite。 */
class AndroidDb private constructor(private val db: SQLiteDatabase) : Db {
    override fun run(sql: String, vararg args: Any?) = db.execSQL(sql, args)

    override fun <T> query(sql: String, vararg args: Any?, map: (Db.Row) -> T): List<T> =
        db.rawQuery(sql, args.map { it?.toString() }.toTypedArray()).use { c ->
            val row = object : Db.Row {
                override fun long(i: Int) = c.getLong(i)
                override fun text(i: Int) = c.getString(i).orEmpty()
                override fun textOrNull(i: Int) = if (c.isNull(i)) null else c.getString(i)
            }
            buildList { while (c.moveToNext()) add(map(row)) }
        }

    override fun <T> transaction(body: () -> T): T {
        db.beginTransaction()
        try {
            return body().also { db.setTransactionSuccessful() }
        } finally {
            db.endTransaction()
        }
    }

    companion object {
        /** 打开数据库。坏了就挪到一边重新建：里面只是缓存，配对信息丢了重新扫码即可，不能让应用打不开。 */
        fun open(context: Context): AndroidDb {
            val file = File(context.noBackupFilesDir, "duolo.sqlite")
            return try {
                create(file)
            } catch (_: Exception) {
                for (suffix in listOf("", "-wal", "-shm", "-journal")) {
                    File(file.path + suffix).renameTo(File("${file.path}.damaged-${System.currentTimeMillis()}$suffix"))
                }
                // 内存数据库一定能打开：手机满了也能看到界面和错误。
                runCatching { create(file) }.getOrElse { AndroidDb(SQLiteDatabase.create(null)) }
            }
        }

        private fun create(file: File): AndroidDb {
            // 默认的处理是发现损坏就把文件删掉，这里要留着。
            val db = SQLiteDatabase.openDatabase(
                file.path, null, SQLiteDatabase.CREATE_IF_NECESSARY or SQLiteDatabase.ENABLE_WRITE_AHEAD_LOGGING,
            ) { broken -> broken.close() }
            db.rawQuery("select count(*) from sqlite_master", null).use { it.moveToFirst() }
            return AndroidDb(db)
        }
    }
}

/** 机密用系统密钥库里的密钥加密后存成文件。密钥出不了这台手机；文件不进备份。 */
class KeystoreSecrets(context: Context) : SecretStore {
    private val dir = File(context.noBackupFilesDir, "secrets").apply { mkdirs() }

    private fun file(computer: String) = File(dir, computer.filter { it.isLetterOrDigit() || it == '-' })

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply { init(spec) }.generateKey()
    }

    override fun load(computer: String): Secrets? {
        val file = file(computer)
        if (!file.exists()) return null
        // 解不开（换了手机、密钥库被清空）等于没有：重新配对。
        return runCatching {
            val data = file.readBytes()
            val cipher = Cipher.getInstance(TRANSFORM).apply { init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, data, 0, 12)) }
            WireJson.decodeFromString<Secrets>(cipher.doFinal(data, 12, data.size - 12).decodeToString())
        }.getOrNull()
    }

    override fun save(secrets: Secrets, computer: String) {
        val cipher = Cipher.getInstance(TRANSFORM).apply { init(Cipher.ENCRYPT_MODE, key()) }
        val sealed = cipher.iv + cipher.doFinal(WireJson.encodeToString(secrets).toByteArray())
        // 先写临时文件再改名，写到一半断电也不会留下半个。
        val temp = File(dir, "${file(computer).name}.tmp")
        temp.outputStream().use {
            it.write(sealed)
            it.fd.sync()
        }
        if (!temp.renameTo(file(computer))) throw java.io.IOException("cannot save secrets")
    }

    override fun remove(computer: String) {
        file(computer).delete()
    }

    private companion object {
        const val ALIAS = "duolo.pairing"
        const val TRANSFORM = "AES/GCM/NoPadding"
    }
}
