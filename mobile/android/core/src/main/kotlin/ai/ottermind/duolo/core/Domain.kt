package ai.ottermind.duolo.core

// 列表和时间线的推导。规则和 iOS 一致，样例在 mobile/contract/behavior.json。

object Excerpt {
    private val marker = Regex("""^(#{1,6}|>|[-*+]|\d+[.)])\s+""")
    private val link = Regex("""\[([^\]]+)]\([^)]*\)""")
    private val sentence = Regex("""[。！？]|[.!?](?=\s)""")

    /** 一段 Markdown 的第一句话，去掉格式。 */
    fun first(markdown: String): String {
        var inCode = false
        for (raw in markdown.lines()) {
            var line = raw.trim()
            if (line.startsWith("```")) {
                inCode = !inCode
                continue
            }
            if (inCode || line.isEmpty()) continue
            while (true) line = line.removeRange(marker.find(line)?.range ?: break)
            line = line.replace(link, "$1")
            for (mark in listOf("**", "`", "~~")) line = line.replace(mark, "")
            if (line.isEmpty()) continue
            return sentence.find(line)?.let { line.substring(0, it.range.last + 1) } ?: line
        }
        return ""
    }
}

/** 列表上的一行。 */
data class TaskRow(val task: TaskOverview) {
    enum class Group { Attention, Active, Done }

    sealed class State(val group: Group) {
        data object NeedsYou : State(Group.Attention)
        data class Issues(val count: Int) : State(Group.Attention)
        data object ReviewFailed : State(Group.Attention)
        data object Crashed : State(Group.Attention)
        data object Writing : State(Group.Active)
        data object Reviewing : State(Group.Active)
        data object Passed : State(Group.Done)
        data object Reviewed : State(Group.Done)
        data object Written : State(Group.Done)
        data object Ended : State(Group.Done)
        data object NotStarted : State(Group.Done)
    }

    val id: Long get() = task.id
    val state: State = stateOf(task)
    val group: Group get() = state.group

    /** 第二行：最能说明现在情况的一句话。 */
    val detail: String = when (state) {
        // 要运行的命令照原样显示第一行，不当成 Markdown。
        State.NeedsYou -> task.prompt?.let { p -> p.detail.ifEmpty { p.question }.lines().firstOrNull { it.isNotEmpty() }.orEmpty() }
            ?: Excerpt.first(task.last)
        is State.Issues, State.ReviewFailed, State.Passed, State.Reviewed -> Excerpt.first(task.review?.summary?.ifEmpty { null } ?: task.last)
        else -> Excerpt.first(task.last)
    }

    /** 进行中的显示做了多久，其他的显示多久以前。 */
    val since: Long?
        get() = when (state) {
            State.Reviewing -> task.reviewingSince
            State.Writing -> task.since.takeIf { it > 0 }
            else -> null
        }

    companion object {
        private fun stateOf(task: TaskOverview): State {
            if (task.status == "needs_input") return State.NeedsYou
            if (task.reviewingSince != null) return State.Reviewing
            if (task.status == "running") return State.Writing
            if (task.status == "failed") return State.Crashed
            // 审阅之后又接着做了：以前的结论不算数。
            val review = task.review
            if (review != null && !(task.status == "idle" && task.since > review.finishedAt)) {
                if (!review.ok) return State.ReviewFailed
                return when (review.verdict) {
                    "issues" -> State.Issues(review.issues)
                    "pass" -> State.Passed
                    else -> State.Reviewed
                }
            }
            return when (task.status) {
                "idle" -> State.Written
                "exited" -> State.Ended
                else -> State.NotStarted
            }
        }

        fun rows(tasks: List<TaskOverview>): List<TaskRow> =
            tasks.map(::TaskRow).sortedWith(compareBy<TaskRow> { it.group }.thenByDescending { it.task.updatedAt }.thenByDescending { it.id })
    }
}

/** 任务详情里的内容，由事件推导。 */
class Timeline(events: List<TaskEvent> = emptyList()) {
    enum class StepKind { Read, Edit, Run, Search, Other }

    data class Step(val seq: Long, val kind: StepKind, val tool: String, val target: String)

    sealed interface Item {
        val seq: Long

        data class User(override val seq: Long, val text: String) : Item
        data class Agent(override val seq: Long, val text: String) : Item

        /** 两句话之间的工具调用合成一组。 */
        data class Steps(val steps: List<Step>) : Item {
            override val seq get() = steps.first().seq
        }

        data class Review(
            override val seq: Long,
            val reviewer: String,
            val ok: Boolean,
            val verdict: String,
            val issues: Int,
            val summary: String,
        ) : Item
    }

    /** 一组里读了、改了几个文件（同一个文件只算一次），运行了几次，其他几次。 */
    data class Counts(val read: Int, val edit: Int, val run: Int, val other: Int) {
        companion object {
            fun of(steps: List<Step>): Counts {
                fun files(kind: StepKind) = steps.filter { it.kind == kind }.map { it.target }.toSet().size
                return Counts(
                    files(StepKind.Read),
                    files(StepKind.Edit),
                    steps.count { it.kind == StepKind.Run },
                    steps.count { it.kind == StepKind.Search || it.kind == StepKind.Other },
                )
            }
        }
    }

    val items: List<Item>

    /** 最后一次工具调用；之后说过话就没有了。 */
    var activity: Step? = null
        private set
    var diff: DiffOverview? = null
        private set
    var closed = false
        private set

    init {
        val list = mutableListOf<Item>()
        val run = mutableListOf<Step>()
        var reviewer = ""
        fun endRun() {
            if (run.isNotEmpty()) list += Item.Steps(run.toList())
            run.clear()
        }
        for (event in events) {
            when (val kind = event.kind) {
                is TaskEvent.Kind.Message -> {
                    endRun()
                    activity = null
                    list += if (kind.fromUser) Item.User(event.seq, kind.text) else Item.Agent(event.seq, kind.text)
                }
                is TaskEvent.Kind.Tool -> Step(event.seq, kindOf(kind.name), kind.name, kind.target).let {
                    activity = it
                    run += it
                }
                is TaskEvent.Kind.ReviewStarted -> reviewer = kind.reviewer
                is TaskEvent.Kind.ReviewFinished -> {
                    endRun()
                    list += Item.Review(event.seq, reviewer, kind.ok, kind.verdict, kind.issues, kind.summary)
                }
                is TaskEvent.Kind.Diff -> diff = kind.stat
                TaskEvent.Kind.TaskClosed -> closed = true
                else -> Unit
            }
        }
        endRun()
        items = list
    }

    val steps: List<Step> get() = items.filterIsInstance<Item.Steps>().flatMap { it.steps }

    private fun kindOf(tool: String) = when (tool.lowercase()) {
        "read", "notebookread", "view_image" -> StepKind.Read
        "edit", "multiedit", "write", "notebookedit", "apply_patch" -> StepKind.Edit
        "bash", "shell", "exec_command", "local_shell", "unified_exec" -> StepKind.Run
        "grep", "glob", "ls", "websearch", "webfetch", "web_search" -> StepKind.Search
        else -> StepKind.Other
    }
}

/** 审阅全文去掉第一行的判断标记（`[ISSUES 2] …`）。 */
fun reviewBody(output: String): String {
    val text = output.trim()
    val end = if (text.startsWith("[")) text.indexOf(']') else -1
    return if (end < 0) text else text.substring(end + 1).trim()
}
