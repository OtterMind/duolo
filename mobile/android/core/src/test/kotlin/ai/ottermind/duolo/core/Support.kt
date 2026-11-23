package ai.ottermind.duolo.core

import kotlinx.coroutines.delay
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import java.io.File
import java.sql.Connection
import java.sql.DriverManager
import java.sql.PreparedStatement

/** 仓库里的文件。三端共用的样例在 mobile/contract/。 */
object Repo {
    val root = File(System.getProperty("duolo.repo"))
    fun json(path: String): JsonObject = WireJson.parseToJsonElement(root.resolve(path).readText()).jsonObject
}

fun JsonElement.at(key: String): JsonElement = (this as JsonObject).getValue(key)

/** 测试用的 SQLite。 */
class JdbcDb(path: String = ":memory:") : Db {
    private val connection: Connection = DriverManager.getConnection("jdbc:sqlite:$path")

    private fun bind(s: PreparedStatement, args: Array<out Any?>) = args.forEachIndexed { i, v -> s.setObject(i + 1, v) }

    override fun run(sql: String, vararg args: Any?) {
        connection.prepareStatement(sql).use {
            bind(it, args)
            it.execute()
        }
    }

    override fun <T> query(sql: String, vararg args: Any?, map: (Db.Row) -> T): List<T> = connection.prepareStatement(sql).use { s ->
        bind(s, args)
        s.executeQuery().use { r ->
            val row = object : Db.Row {
                override fun long(i: Int) = r.getLong(i + 1)
                override fun text(i: Int): String = r.getString(i + 1).orEmpty()
                override fun textOrNull(i: Int): String? = r.getString(i + 1)
            }
            buildList { while (r.next()) add(map(row)) }
        }
    }

    override fun <T> transaction(body: () -> T): T {
        connection.autoCommit = false
        try {
            return body().also { connection.commit() }
        } catch (e: Throwable) {
            connection.rollback()
            throw e
        } finally {
            connection.autoCommit = true
        }
    }

    fun close() = connection.close()
}

class MemorySecrets : SecretStore {
    private val items = java.util.concurrent.ConcurrentHashMap<String, Secrets>()
    override fun load(computer: String) = items[computer]
    override fun save(secrets: Secrets, computer: String) {
        items[computer] = secrets
    }
    override fun remove(computer: String) {
        items.remove(computer)
    }
}

suspend fun until(what: String, timeout: Long = 3000, done: suspend () -> Boolean) {
    val deadline = System.currentTimeMillis() + timeout
    while (!done()) {
        if (System.currentTimeMillis() > deadline) throw AssertionError("timed out waiting for $what")
        delay(5)
    }
}
