package ai.ottermind.duolo.core

import java.io.InputStream
import java.util.Locale

/** 多语言。文案和桌面端共用仓库根目录的 locales/<code>.json（规范见 docs/i18n.md），原样打包进应用。 */
class Localizer(val code: String, open: (String) -> InputStream) {
    private val base: Map<String, String> = load("en", open)
    private val messages: Map<String, String> = if (code == "en") base else load(code, open)

    /** 系统的日期、数字按这个语言显示。 */
    val locale: Locale = Locale.forLanguageTag(if (code == "zh-CN") "zh-Hans-CN" else if (code == "zh-TW") "zh-Hant-TW" else code)

    private fun load(code: String, open: (String) -> InputStream): Map<String, String> = runCatching {
        WireJson.decodeFromString<Map<String, String>>(open("$code.json").use { it.readBytes().decodeToString() })
    }.getOrDefault(emptyMap())

    /** 找不到时退回英文，再找不到返回键名。`{name}` 换成 `vars` 里的值。 */
    operator fun invoke(key: String, vararg vars: Pair<String, Any>): String {
        var text = messages[key] ?: base[key] ?: key
        for ((name, value) in vars) text = text.replace("{$name}", value.toString())
        return text
    }

    /** 错误码对应的说明。 */
    fun error(code: String): String = if ("error.$code" in base) this("error.$code") else this("error.internal", "message" to code)

    companion object {
        /** 支持的语言，名字用该语言自己的写法。与 app/src/i18n/index.tsx 的 `LOCALES` 一致。 */
        val locales = listOf(
            "en" to "English", "zh-CN" to "简体中文", "zh-TW" to "繁體中文", "ja" to "日本語", "ko" to "한국어",
            "es" to "Español", "fr" to "Français", "de" to "Deutsch", "pt-BR" to "Português (Brasil)", "ru" to "Русский",
            "it" to "Italiano", "tr" to "Türkçe", "vi" to "Tiếng Việt", "id" to "Bahasa Indonesia",
        )

        /** 系统语言标签对应到支持的语言，规则与桌面端的 `match_locale` 一致。 */
        fun match(tags: List<String>): String {
            for (raw in tags) {
                val tag = raw.lowercase(Locale.ROOT).replace('_', '-')
                if (tag.startsWith("zh")) return if (listOf("hant", "tw", "hk", "mo").any { it in tag }) "zh-TW" else "zh-CN"
                if (tag.startsWith("pt")) return "pt-BR"
                // Java 里印尼语还用旧代码。
                val lang = tag.substringBefore('-').let { if (it == "in") "id" else it }
                locales.firstOrNull { it.first.lowercase(Locale.ROOT) == lang }?.let { return it.first }
            }
            return "en"
        }
    }
}
