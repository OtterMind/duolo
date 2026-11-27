package io.duolo.app.core

/**
 * 识别前给识别服务的提示：热词（文件名、代码里的名字，重要的在前）和最近的对话（新的在前）。
 * 和电脑端的 crates/duolo-voice/src/hints.rs 一致。
 */
data class VoiceHints(val hotwords: List<String> = emptyList(), val context: List<String> = emptyList()) {
    /**
     * 按热词改正识别结果里拆开说、或大小写不对的名字："加个 use state" → "加个 useState"。
     * 只改由几段拼成的名字，几段之间只能隔着空格、`_`、`-`、`.`、`/`。
     */
    fun fixNames(text: String): String {
        // 名字压成小写字母数字作比较；长的先比，免得短的抢了长的（sortedByDescending 是稳定的）。
        val names = hotwords.mapNotNull { word ->
            val key = word.filter(::isAlnum).lowercase()
            val parts = wordParts(word)
            if (parts >= 2 && key.length >= 5) Triple(key, word, parts) else null
        }.sortedByDescending { it.first.length }
        if (names.isEmpty()) return text
        // 文字里的英文、数字段（字符位置；非 ASCII 的字不算，所以和按字节算的结果一样）。
        val runs = mutableListOf<IntRange>()
        var start = -1
        for (i in 0..text.length) {
            val alnum = i < text.length && isAlnum(text[i])
            if (alnum && start < 0) start = i
            else if (!alnum && start >= 0) {
                runs += start until i
                start = -1
            }
        }
        fun gapOk(from: Int, to: Int) = (from until to).all { text[it] in " _-./" }
        val out = StringBuilder()
        var copied = 0
        var i = 0
        scan@ while (i < runs.size) {
            // 前一个字符是 `_` 说明这段是别的名字的一部分。
            if (runs[i].first > 0 && text[runs[i].first - 1] == '_') {
                i++
                continue
            }
            val first = i
            for ((key, name, parts) in names) {
                val joined = StringBuilder()
                for (j in first until minOf(runs.size, first + parts + 1)) {
                    if (j > first && !gapOk(runs[j - 1].last + 1, runs[j].first)) break
                    joined.append(text.substring(runs[j].first, runs[j].last + 1).lowercase())
                    if (joined.length > key.length || !key.startsWith(joined)) break
                    val from = runs[first].first
                    val to = runs[j].last + 1
                    if (joined.toString() == key && !(to < text.length && text[to] == '_')) {
                        if (text.substring(from, to) != name) {
                            out.append(text, copied, from).append(name)
                            copied = to
                        }
                        i = j + 1
                        continue@scan
                    }
                }
            }
            i++
        }
        return out.append(text, copied, text.length).toString()
    }

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
                    is TaskEvent.Kind.Message -> {
                        // Agent 用反引号标出来的最可靠，先放；再是没加反引号但一看就是代码的（双方说的都算）。
                        if (!kind.fromUser) kind.text.split('`').filterIndexed { i, _ -> i % 2 == 1 }.forEach(::push)
                        codeNames(kind.text).forEach(::push)
                    }
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

        /** 文字里看着像代码的名字：`exportCsv`、`snake_case`、`orders.ts`、`duolo_core::t`。普通的英文单词不算。 */
        internal fun codeNames(text: String): List<String> =
            text.split(Regex("[^A-Za-z0-9_\\-./:]+")).map { it.trim('-', '.', '/', ':') }.filter(::isCodeName)

        private fun isCodeName(word: String): Boolean {
            if (word.length < 3 || "://" in word || !(isAlpha(word[0]) || word[0] == '_')) return false
            val camel = word.zipWithNext().any { (a, b) -> a in 'a'..'z' && b in 'A'..'Z' }
            val joined = (1 until word.length - 1).any { word[it] in "_./:" && isAlnum(word[it - 1]) && isAlnum(word[it + 1]) }
            return camel || joined || "::" in word
        }

        /** 名字由几段组成：`useState` 2、`export_csv_v2` 3、`orders.ts` 2。 */
        internal fun wordParts(word: String): Int {
            var parts = 0
            var prev: Char? = null
            for (c in word) {
                val starts = when {
                    !isAlnum(c) -> false
                    prev == null -> true
                    else -> !isAlnum(prev) || (prev in 'a'..'z' && c in 'A'..'Z')
                }
                if (starts) parts++
                prev = c
            }
            return parts
        }

        private fun isAlpha(c: Char) = c in 'a'..'z' || c in 'A'..'Z'
        private fun isAlnum(c: Char) = isAlpha(c) || c in '0'..'9'
    }
}
