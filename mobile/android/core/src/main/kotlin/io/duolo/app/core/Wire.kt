package io.duolo.app.core

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

// 电脑和手机之间的消息，和 crates/duolo-core/src/wire.rs、event.rs 一致。

/** 消息格式的版本。 */
const val WIRE_VERSION = 1

/** 多出来的字段忽略、缺了的用默认值，新旧版本可以互通。 */
val WireJson = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
    explicitNulls = true
}

/** `color` 是图标的颜色（插件的 `icon_color`，`#rrggbb`）；没有时用文字的颜色。 */
@Serializable
data class AgentOption(val kind: String, val name: String, val color: String? = null)

@Serializable
data class ProjectOverview(val id: String, val name: String, val branch: String = "", val branches: List<String> = emptyList())

/** `seq` 是这条提示的事件编号，回答时带回去。 */
@Serializable
data class PromptOverview(val seq: Long, val question: String, val detail: String = "", val options: List<String>)

@Serializable
data class ReviewOverview(val finishedAt: Long, val ok: Boolean, val verdict: String, val issues: Int = 0, val summary: String = "")

@Serializable
data class DiffOverview(val files: Int, val insertions: Int, val deletions: Int)

@Serializable
data class TaskOverview(
    val id: Long,
    val project: String,
    val title: String,
    val untitled: Boolean = false,
    val branch: String = "",
    val status: String,
    /** 列表上显示的状态，电脑按 crates/duolo-core/src/wire.rs 的 `TaskState` 算好；不认识的当作未启动。 */
    val state: String = "not_started",
    /** 改的是项目里的哪个仓库；项目本身就是仓库时为空。 */
    val repo: String = "",
    val writer: AgentOption,
    val reviewer: AgentOption,
    val updatedAt: Long,
    val since: Long = 0,
    val last: String = "",
    val prompt: PromptOverview? = null,
    val reviewingSince: Long? = null,
    val review: ReviewOverview? = null,
    val diff: DiffOverview? = null,
    val seq: Long = 0,
)

@Serializable
data class Overview(
    val rev: Long,
    val projects: List<ProjectOverview> = emptyList(),
    val tasks: List<TaskOverview> = emptyList(),
    val agents: List<AgentOption> = emptyList(),
    val writer: String = "",
    val reviewer: String = "",
    /** 电脑上的语音设置，手机说话时照着用。 */
    val voice: VoiceOverview = VoiceOverview(),
)

/** 语音设置：要认准的词、说完后是否整理成指令（电脑端设置 → 语音）。 */
@Serializable
data class VoiceOverview(val organize: Boolean = false, val words: List<String> = emptyList())

/** 任务里发生的一件事。`raw` 是收到时的原样，保存的是它，升级之后能重新读出以前不认识的。 */
data class TaskEvent(val task: Long, val seq: Long, val at: Long, val kind: Kind, val raw: JsonObject) {
    sealed interface Kind {
        data class TaskCreated(val title: String, val branch: String) : Kind
        data class TaskRenamed(val title: String, val branch: String) : Kind
        data object TaskClosed : Kind
        data class Message(val fromUser: Boolean, val text: String) : Kind
        data class Tool(val name: String, val target: String) : Kind
        data class Status(val status: String) : Kind
        data class Prompt(val question: String, val detail: String, val options: List<String>) : Kind
        data class ReviewStarted(val reviewer: String) : Kind
        data class ReviewFinished(val ok: Boolean, val verdict: String, val issues: Int, val summary: String) : Kind
        data class Diff(val stat: DiffOverview) : Kind

        /** 更新版本的电脑发来的、这个版本还不认识的事件，或者字段对不上的。 */
        data object Unknown : Kind
    }

    companion object {
        fun parse(raw: JsonObject) = TaskEvent(
            task = raw.getValue("task").jsonPrimitive.long,
            seq = raw.getValue("seq").jsonPrimitive.long,
            at = raw["at"]?.jsonPrimitive?.long ?: 0,
            kind = runCatching { kind(raw) }.getOrDefault(Kind.Unknown),
            raw = raw,
        )

        private fun kind(o: JsonObject): Kind {
            fun text(key: String) = o.getValue(key).jsonPrimitive.also { require(it.isString) }.content
            return when (text("type")) {
                "task_created" -> Kind.TaskCreated(text("title"), text("branch"))
                "task_renamed" -> Kind.TaskRenamed(text("title"), text("branch"))
                "task_closed" -> Kind.TaskClosed
                "message" -> Kind.Message(text("speaker") == "user", text("text"))
                "tool" -> Kind.Tool(text("tool"), text("target"))
                "status" -> Kind.Status(text("status"))
                "prompt" -> Kind.Prompt(text("question"), text("detail"), o.getValue("options").jsonArray.map { it.jsonPrimitive.content })
                "review_started" -> Kind.ReviewStarted(text("reviewer"))
                "review_finished" -> Kind.ReviewFinished(
                    o.getValue("ok").jsonPrimitive.content.toBooleanStrict(),
                    text("verdict"),
                    o.getValue("issues").jsonPrimitive.int,
                    text("summary"),
                )
                "diff" -> Kind.Diff(WireJson.decodeFromJsonElement<DiffOverview>(o))
                else -> Kind.Unknown
            }
        }
    }
}

/** 让电脑做的事。 */
@OptIn(ExperimentalSerializationApi::class)
@Serializable
sealed interface Command {
    @Serializable
    @SerialName("create_task")
    data class CreateTask(
        val project: String,
        val prompt: String,
        // 仓库和分支名由电脑按默认值定。
        @EncodeDefault val repo: String? = null,
        @EncodeDefault val base: String? = null,
        @EncodeDefault val branch: String? = null,
        @EncodeDefault val writer: String? = null,
        @EncodeDefault val reviewer: String? = null,
    ) : Command

    @Serializable
    @SerialName("send_message")
    data class SendMessage(override val task: Long, val text: String) : Command

    @Serializable
    @SerialName("answer")
    data class Answer(override val task: Long, val option: Int, val prompt: Long) : Command

    @Serializable
    @SerialName("interrupt")
    data class Interrupt(override val task: Long) : Command

    @Serializable
    @SerialName("start_review")
    data class StartReview(override val task: Long) : Command

    @Serializable
    @SerialName("close_task")
    data class CloseTask(override val task: Long) : Command

    val task: Long? get() = null

    /** 只对当下有意义：电脑不在线就不发，也不排队，晚到的"允许"可能批准另一件事。 */
    val isLive: Boolean get() = this is Answer || this is Interrupt
}

@Serializable
data class CommandResult(val ok: Boolean, val task: Long? = null, val error: String? = null)

/** 手机发给电脑的。 */
@Serializable
sealed interface PhoneMessage {
    @Serializable
    @SerialName("sync")
    data object Sync : PhoneMessage

    @Serializable
    @SerialName("fetch")
    data class Fetch(val task: Long, val after: Long) : PhoneMessage

    @Serializable
    @SerialName("command")
    data class Run(val id: String, val at: Long, val command: Command) : PhoneMessage

    fun toJson(): JsonObject {
        val body = WireJson.encodeToJsonElement<PhoneMessage>(this).jsonObject
        return buildJsonObject {
            put("v", WIRE_VERSION)
            body.forEach { (key, value) -> put(key, value) }
        }
    }
}

/** 电脑发给手机的。 */
sealed interface DesktopMessage {
    data class OverviewChanged(val overview: Overview) : DesktopMessage
    data class Events(val task: Long, val events: List<TaskEvent>, val latest: Long) : DesktopMessage
    data class Review(val task: Long, val finishedAt: Long, val output: String) : DesktopMessage
    data class Result(val id: String, val result: CommandResult) : DesktopMessage

    /** 这个版本还不认识的消息，忽略。 */
    data object Unknown : DesktopMessage

    companion object {
        fun parse(json: JsonElement): DesktopMessage {
            val o = json.jsonObject
            fun number(key: String) = o.getValue(key).jsonPrimitive.long
            fun text(key: String) = o.getValue(key).jsonPrimitive.content
            return when ((o["type"] as? JsonPrimitive)?.contentOrNull) {
                "overview" -> OverviewChanged(WireJson.decodeFromJsonElement(o))
                "events" -> Events(number("task"), o.getValue("events").jsonArray.map { TaskEvent.parse(it.jsonObject) }, number("latest"))
                "review" -> Review(number("task"), number("finishedAt"), text("output"))
                "result" -> Result(text("id"), WireJson.decodeFromJsonElement(o.getValue("result")))
                else -> Unknown
            }
        }
    }
}
