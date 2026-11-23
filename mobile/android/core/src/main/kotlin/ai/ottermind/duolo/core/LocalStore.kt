package ai.ottermind.duolo.core

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.jsonObject
import java.util.Base64

/** SQLite。应用里是系统自带的，测试里是 JDBC；SQL 只用 Android 8 的 SQLite（3.18）就有的写法。 */
interface Db {
    interface Row {
        fun long(i: Int): Long
        fun text(i: Int): String
        fun textOrNull(i: Int): String?
    }

    /** 参数只有 Long、String、null。 */
    fun run(sql: String, vararg args: Any?)
    fun <T> query(sql: String, vararg args: Any?, map: (Row) -> T): List<T>

    /** 全部成功才算，中途失败就当没做过。 */
    fun <T> transaction(body: () -> T): T
}

/** 配对过的一台电脑。令牌和密钥在 `SecretStore` 里，不在数据库里。 */
data class Computer(
    val id: String,
    val name: String,
    val platform: String,
    val server: String,
    /** 这台手机在服务器上的设备编号。 */
    val deviceId: String,
    /** 电脑的公钥，base64。 */
    val desktopKey: String,
)

/** 还没有结果的命令，或者没成功的。成功的就删掉了。 */
data class OutboxItem(val id: String, val command: Command, val at: Long, val state: State, val error: String? = null) {
    enum class State { queued, sent, failed }
}

/** 手机上保存的内容。所有读写排队进行；写入都在事务里，要么全部成功要么没做过。 */
class LocalStore(private val db: Db) {
    private val lock = Any()

    init {
        migrate()
    }

    /** 数据库的版本只往上加，每一步都能从上一步升级。 */
    private fun migrate() {
        val steps = listOf(
            listOf(
                """create table computer (
                  id text primary key, name text not null, platform text not null, server text not null,
                  device_id text not null, desktop_key text not null, added_at integer not null)""",
                "create table overview (computer text primary key, rev integer not null, json text not null)",
                """create table event (
                  computer text not null, task integer not null, seq integer not null, json text not null, primary key (computer, task, seq))""",
                """create table review (
                  computer text not null, task integer not null, finished_at integer not null, output text not null, primary key (computer, task))""",
                """create table outbox (
                  id text primary key, computer text not null, json text not null, at integer not null, state text not null, error text)""",
            ),
        )
        steps.forEachIndexed { i, statements ->
            val version = db.query("pragma user_version") { it.long(0) }.firstOrNull() ?: 0
            if (version > i) return@forEachIndexed
            db.transaction {
                statements.forEach { db.run(it) }
                db.run("pragma user_version = ${i + 1}")
            }
        }
    }

    private inline fun <T> locked(body: () -> T): T = synchronized(lock, body)

    // ---- 电脑 ----

    fun computers(): List<Computer> = locked {
        db.query("select id, name, platform, server, device_id, desktop_key from computer order by added_at, id") {
            Computer(it.text(0), it.text(1), it.text(2), it.text(3), it.text(4), it.text(5))
        }
    }

    /** 加一台电脑。重新配对同一台时换成新的，以前的任务缓存留着，没送出的命令作废（旧设备已经失效）。 */
    fun add(c: Computer, now: Long) = locked {
        db.transaction {
            db.run("delete from outbox where computer = ?", c.id)
            val known = db.query("select 1 from computer where id = ?", c.id) { it.long(0) }.isNotEmpty()
            if (known) {
                db.run(
                    "update computer set name = ?, platform = ?, server = ?, device_id = ?, desktop_key = ? where id = ?",
                    c.name, c.platform, c.server, c.deviceId, c.desktopKey, c.id,
                )
            } else {
                db.run(
                    "insert into computer (id, name, platform, server, device_id, desktop_key, added_at) values (?, ?, ?, ?, ?, ?, ?)",
                    c.id, c.name, c.platform, c.server, c.deviceId, c.desktopKey, now,
                )
            }
        }
    }

    fun rename(computer: String, name: String) = locked { db.run("update computer set name = ? where id = ?", name, computer) }

    fun remove(computer: String) = locked {
        db.transaction {
            for (table in listOf("overview", "event", "review", "outbox")) db.run("delete from $table where computer = ?", computer)
            db.run("delete from computer where id = ?", computer)
        }
    }

    // ---- 总览 ----

    fun overview(computer: String): Overview? = locked {
        db.query("select json from overview where computer = ?", computer) { it.text(0) }.firstOrNull()
            ?.let { runCatching { WireJson.decodeFromString<Overview>(it) }.getOrNull() }
    }

    /** 换成新的总览；比现有的旧就不动，返回 false。已经没有的任务，它的事件一起清掉。 */
    fun save(overview: Overview, computer: String): Boolean {
        val json = WireJson.encodeToString(overview)
        return locked {
            db.transaction {
                val current = db.query("select rev from overview where computer = ?", computer) { it.long(0) }.firstOrNull()
                if (current != null && current >= overview.rev) return@transaction false
                db.run("insert or replace into overview (computer, rev, json) values (?, ?, ?)", computer, overview.rev, json)
                val keep = overview.tasks.joinToString(",") { it.id.toString() }
                for (table in listOf("event", "review")) db.run("delete from $table where computer = ? and task not in ($keep)", computer)
                true
            }
        }
    }

    // ---- 事件 ----

    sealed interface Append {
        /** 接上了；`more` 为真时电脑上还有后面的。 */
        data class Applied(val more: Boolean) : Append

        /** 中间缺了，要从 `after` 之后重新要。 */
        data class Gap(val after: Long) : Append
    }

    fun lastSeq(computer: String, task: Long): Long = locked { last(computer, task) }

    private fun last(computer: String, task: Long): Long =
        db.query("select coalesce(max(seq), 0) from event where computer = ? and task = ?", computer, task) { it.long(0) }.firstOrNull() ?: 0

    /** 存事件。只接连续的；电脑上最后的编号比手机上的还小，说明那边重来过，手机上的作废。 */
    fun append(events: List<TaskEvent>, task: Long, latest: Long, computer: String): Append {
        val rows = events.sortedBy { it.seq }.map { it.seq to it.raw.toString() }
        return locked {
            db.transaction {
                var last = last(computer, task)
                if (latest < last) {
                    db.run("delete from event where computer = ? and task = ?", computer, task)
                    last = 0
                    if (rows.firstOrNull()?.first != 1L) return@transaction Append.Gap(0)
                }
                for ((seq, json) in rows) {
                    if (seq <= last) continue
                    if (last > 0 && seq != last + 1) return@transaction Append.Gap(last)
                    db.run("insert or ignore into event (computer, task, seq, json) values (?, ?, ?, ?)", computer, task, seq, json)
                    last = seq
                }
                Append.Applied(last < latest)
            }
        }
    }

    fun events(computer: String, task: Long): List<TaskEvent> = locked {
        db.query("select json from event where computer = ? and task = ? order by seq", computer, task) { it.text(0) }
    }.mapNotNull { runCatching { TaskEvent.parse(WireJson.parseToJsonElement(it).jsonObject) }.getOrNull() }

    // ---- 审阅全文 ----

    fun saveReview(output: String, finishedAt: Long, task: Long, computer: String) = locked {
        db.run("insert or replace into review (computer, task, finished_at, output) values (?, ?, ?, ?)", computer, task, finishedAt, output)
    }

    fun review(computer: String, task: Long): String? = locked {
        db.query("select output from review where computer = ? and task = ?", computer, task) { it.text(0) }.firstOrNull()
    }

    // ---- 待发送 ----

    fun enqueue(item: OutboxItem, computer: String) {
        val json = WireJson.encodeToString<Command>(item.command)
        locked {
            db.run(
                "insert or replace into outbox (id, computer, json, at, state, error) values (?, ?, ?, ?, ?, ?)",
                item.id, computer, json, item.at, item.state.name, item.error,
            )
        }
    }

    fun outbox(computer: String): List<OutboxItem> = locked {
        db.query("select id, json, at, state, error from outbox where computer = ? order by at, id", computer) {
            runCatching {
                OutboxItem(it.text(0), WireJson.decodeFromString(it.text(1)), it.long(2), OutboxItem.State.valueOf(it.text(3)), it.textOrNull(4))
            }.getOrNull()
        }.filterNotNull()
    }

    fun mark(id: String, state: OutboxItem.State, error: String? = null) =
        locked { db.run("update outbox set state = ?, error = ? where id = ?", state.name, error, id) }

    fun dequeue(id: String) = locked { db.run("delete from outbox where id = ?", id) }
}

/** 一台电脑的机密：登录令牌、这台手机的私钥、配对时的一次性密码（后两个是 base64）。 */
@kotlinx.serialization.Serializable
data class Secrets(val token: String, val privateKey: String, val secret: String) {
    fun privateKeyBytes(): ByteArray = Base64.getDecoder().decode(privateKey)
    fun secretBytes(): ByteArray = Base64.getDecoder().decode(secret)
}

interface SecretStore {
    fun load(computer: String): Secrets?
    fun save(secrets: Secrets, computer: String)
    fun remove(computer: String)
}
