package ai.ottermind.duolo.core

/**
 * 识别前给识别服务的提示：热词（文件名、代码里的名字，重要的在前）和最近的对话（新的在前）。
 * 和电脑端的 crates/duolo-voice/src/hints.rs 一致。
 */
data class VoiceHints(val hotwords: List<String> = emptyList(), val context: List<String> = emptyList()) {
    companion object {
        private const val MAX_HOTWORDS = 40
        private const val MAX_CONTEXT = 6
        private const val MAX_CONTEXT_CHARS = 300
        private const val MAX_WORD_CHARS = 40

        /** `events` 旧的在前。 */
        fun of(project: String?, title: String?, branch: String?, events: List<TaskEvent>): VoiceHints {
            val words = mutableListOf<String>()
            fun push(raw: String) {
                val word = raw.trim().trim('"', '\'', ',', '.', ':', ';', '(', ')')
                val ok = word.isNotEmpty() && word.length <= MAX_WORD_CHARS && word.none { it.isWhitespace() } && word.any { it.isLetter() }
                if (ok && words.size < MAX_HOTWORDS && word !in words) words += word
            }
            for (event in events.asReversed()) {
                when (val kind = event.kind) {
                    is TaskEvent.Kind.Tool -> if (looksLikePath(kind.target)) push(kind.target.split('/', '\\').last())
                    is TaskEvent.Kind.Message -> if (!kind.fromUser) kind.text.split('`').filterIndexed { i, _ -> i % 2 == 1 }.forEach(::push)
                    else -> {}
                }
            }
            listOfNotNull(branch, project).forEach(::push)

            val dialog = mutableListOf<String>()
            for (event in events.asReversed()) {
                val text = when (val kind = event.kind) {
                    is TaskEvent.Kind.Prompt ->
                        if (kind.options.isEmpty()) kind.question else "${kind.question} (${kind.options.joinToString(" / ")})"
                    is TaskEvent.Kind.Message -> kind.text
                    else -> continue
                }.trim().takeLast(MAX_CONTEXT_CHARS)
                if (text.isNotEmpty() && text !in dialog) dialog += text
                if (dialog.size >= MAX_CONTEXT) break
            }
            title?.trim()?.takeIf { it.isNotEmpty() }?.let { dialog += it }
            return VoiceHints(words, dialog)
        }

        /** 工具的目标是文件路径（不是一条命令）。 */
        internal fun looksLikePath(target: String) =
            target.isNotEmpty() && target.none { it.isWhitespace() } && (target.contains('/') || target.contains('\\') || target.contains('.'))
    }
}
