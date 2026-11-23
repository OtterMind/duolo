package ai.ottermind.duolo

import ai.ottermind.duolo.core.AgentOption
import ai.ottermind.duolo.core.Localizer
import ai.ottermind.duolo.core.TaskRow
import android.icu.text.DisplayContext
import android.icu.text.MeasureFormat
import android.icu.text.RelativeDateTimeFormatter
import android.icu.util.Measure
import android.icu.util.MeasureUnit
import android.icu.util.ULocale
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable

/** 颜色，浅色和深色成对。和 iOS 的 Theme.swift 一致。 */
@Immutable
data class Palette(
    val bg: Color, val raised: Color, val bubble: Color, val button: Color, val text: Color, val muted: Color, val dim: Color, val line: Color,
    /** 绿色按钮上的字。 */
    val onGreen: Color,
    /** 写成文字的绿：浅色主题用深一档，不然看不清。 */
    val greenText: Color,
    val greenSoft: Color,
    val wait: Color, val bad: Color, val write: Color, val review: Color, val gray: Color,
) {
    val green = Color(0xFF07C160)

    companion object {
        val Light = Palette(
            bg = Color(0xFFFFFFFF), raised = Color(0xFFF3F5F7), bubble = Color(0xFFEEF1F4), button = Color(0xFFE1E5EA), text = Color(0xFF1C2127),
            muted = Color(0xFF57606E), dim = Color(0xFF66707E), line = Color(0xFFE7EAEE), onGreen = Color(0xFF04210F), greenText = Color(0xFF007A3A),
            greenSoft = Color(0xFFE6F8EF), wait = Color(0xFFA15C00), bad = Color(0xFFCF2F4A), write = Color(0xFF0B6BCB), review = Color(0xFF7A4FD6),
            gray = Color(0xFF5F6B7A),
        )
        val Dark = Palette(
            bg = Color(0xFF161616), raised = Color(0xFF222222), bubble = Color(0xFF262626), button = Color(0xFF2E2E2E), text = Color(0xFFE4E4E4),
            muted = Color(0xFFA3A3A3), dim = Color(0xFF8C8C8C), line = Color(0xFF2A2A2A), onGreen = Color(0xFF04210F), greenText = Color(0xFF07C160),
            greenSoft = Color(0xFF14301F), wait = Color(0xFFFFC24B), bad = Color(0xFFFF7A85), write = Color(0xFF5CB8FF), review = Color(0xFFB69CFF),
            gray = Color(0xFF9AA4B2),
        )
    }
}

private val LocalPalette = staticCompositionLocalOf { Palette.Light }
val LocalModel = staticCompositionLocalOf<AppModel> { error("no model") }

val palette: Palette @Composable get() = LocalPalette.current

@OptIn(ExperimentalTextApi::class)
private fun mona(weight: Int) = Font(R.font.mona_sans, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight)))

/** Mona Sans；没有的字（中日韩等）系统自动换成自己的字体。 */
val MonaSans = FontFamily(mona(400), mona(500), mona(600))

@Composable
fun DuoloTheme(appearance: String, content: @Composable () -> Unit) {
    val dark = when (appearance) {
        "light" -> false
        "dark" -> true
        else -> isSystemInDarkTheme()
    }
    val p = if (dark) Palette.Dark else Palette.Light
    // Material 的弹窗、菜单也用同一套颜色。
    val scheme = (if (dark) darkColorScheme() else lightColorScheme()).copy(
        primary = p.greenText, onPrimary = p.onGreen, background = p.bg, onBackground = p.text, surface = p.bg, onSurface = p.text,
        onSurfaceVariant = p.muted, surfaceContainer = p.raised, surfaceContainerHigh = p.raised, surfaceContainerLow = p.bg, outline = p.line,
        outlineVariant = p.line, error = p.bad, secondaryContainer = p.greenSoft, onSecondaryContainer = p.greenText,
    )
    MaterialTheme(colorScheme = scheme) {
        CompositionLocalProvider(
            LocalPalette provides p,
            LocalContentColor provides p.text,
            LocalTextStyle provides TextStyle(fontFamily = MonaSans, fontSize = 16.sp, lineHeight = 24.sp, color = p.text),
            LocalTextSelectionColors provides TextSelectionColors(p.greenText, p.green.copy(alpha = 0.3f)),
            content = content,
        )
    }
}

/** 一段文字。 */
@Composable
fun Label(
    text: String,
    modifier: Modifier = Modifier,
    size: TextUnit = 16.sp,
    weight: FontWeight = FontWeight.Normal,
    color: Color = palette.text,
    maxLines: Int = Int.MAX_VALUE,
    align: TextAlign? = null,
    style: TextStyle = LocalTextStyle.current,
) = Text(
    text, modifier, color = color, fontSize = size, fontWeight = weight, lineHeight = size * 1.5f, maxLines = maxLines,
    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis, textAlign = align, style = style,
)

/** GitHub Octicons。 */
@Composable
fun Icon(id: Int, modifier: Modifier = Modifier, size: Dp = 18.dp, tint: Color = palette.text) =
    Image(painterResource(id), null, modifier.size(size), colorFilter = ColorFilter.tint(tint))

/** 图标按钮：点按区 48×48，读屏时念 `label`。 */
@Composable
fun IconButton(id: Int, label: String, modifier: Modifier = Modifier, tint: Color = palette.text, enabled: Boolean = true, onClick: () -> Unit) = Box(
    modifier.size(48.dp).clip(CircleShape).clickable(enabled, role = Role.Button, onClick = onClick).semantics { contentDescription = label }
        .alpha(if (enabled) 1f else 0.4f),
    Alignment.Center,
) { Icon(id, tint = tint) }

@Composable
fun BigButton(text: String, modifier: Modifier = Modifier, primary: Boolean = true, enabled: Boolean = true, onClick: () -> Unit) = Box(
    modifier.fillMaxWidth().defaultMinSize(minHeight = 50.dp).clip(RoundedCornerShape(25.dp))
        .background(if (primary) palette.green else palette.button)
        .clickable(enabled, role = Role.Button, onClick = onClick)
        .alpha(if (enabled) 1f else 0.45f)
        .padding(horizontal = 16.dp, vertical = 12.dp),
    Alignment.Center,
) { Label(text, weight = FontWeight.Medium, color = if (primary) palette.onGreen else palette.text, align = TextAlign.Center) }

/** `#rrggbb`，读不出来时为空。 */
fun hexColor(hex: String?): Color? =
    hex?.takeIf { it.length == 7 && it.startsWith("#") }?.substring(1)?.toLongOrNull(16)?.let { Color(0xFF000000 or it) }

/** 写代码的 Agent 的图标：插件自带的（mobile/scripts/gen-agent-icons.mjs 生成），颜色由电脑发来；没有图标的显示首字母。 */
@Composable
fun AgentMark(agent: AgentOption, size: Dp = 14.dp) {
    val icon = agentIcons[agent.kind]
    val described = Modifier.semantics { contentDescription = agent.name }
    if (icon != null) {
        Icon(icon, described, size, hexColor(agent.color) ?: palette.muted)
    } else {
        Box(described.size(size).clip(RoundedCornerShape(3.dp)).background(palette.button), Alignment.Center) {
            Text(agent.kind.take(1).uppercase(), color = palette.muted, fontSize = (size.value * 0.7f).sp, fontWeight = FontWeight.SemiBold)
        }
    }
}

/** 项目里的哪个仓库。 */
@Composable
fun RepoTag(name: String) = Label(
    name, Modifier.clip(RoundedCornerShape(5.dp)).background(palette.raised).padding(horizontal = 6.dp, vertical = 1.dp),
    size = 12.sp, color = palette.muted, maxLines = 1,
)

/** 任务状态的颜色、图标和名字，列表和详情都用这一份。 */
data class StatusStyle(val icon: Int, val color: Color, val label: String, val active: Boolean)

@Composable
fun statusStyle(state: TaskRow.State, t: Localizer): StatusStyle {
    val p = palette
    val (icon, color) = when (state) {
        TaskRow.State.NeedsYou -> R.drawable.ic_question to p.wait
        is TaskRow.State.Issues, TaskRow.State.ReviewFailed -> R.drawable.ic_issue_opened to p.bad
        TaskRow.State.Crashed -> R.drawable.ic_x_circle to p.bad
        TaskRow.State.Writing -> R.drawable.ic_pencil to p.write
        TaskRow.State.Reviewing -> R.drawable.ic_eye to p.review
        TaskRow.State.Passed -> R.drawable.ic_check_circle to p.greenText
        TaskRow.State.Reviewed, TaskRow.State.Written -> R.drawable.ic_check_circle to p.gray
        TaskRow.State.Ended -> R.drawable.ic_skip to p.gray
        TaskRow.State.NotStarted -> R.drawable.ic_circle to p.gray
    }
    // 文案和电脑共用 state.*。
    val label = t("state.${state.code}", "count" to ((state as? TaskRow.State.Issues)?.count ?: 0))
    return StatusStyle(icon, color, label, state.group == TaskRow.Group.Active)
}

/** 状态图标：同样的圆角底，只换颜色和图标；进行中的会轻轻闪（系统关了动画就不闪）。 */
@Composable
fun StatusBadge(style: StatusStyle, size: Dp = 36.dp) = Box(
    Modifier.size(size).clip(RoundedCornerShape(size * 0.3f)).background(style.color.copy(alpha = 0.13f)),
    Alignment.Center,
) {
    val alpha = if (style.active) {
        val pulse by rememberInfiniteTransition(label = "pulse").animateFloat(1f, 0.45f, infiniteRepeatable(tween(900), RepeatMode.Reverse), "alpha")
        pulse
    } else {
        1f
    }
    Icon(style.icon, Modifier.alpha(alpha), size / 2, style.color)
}

object Times {
    /** "3 小时前"。 */
    fun ago(t: Localizer, ms: Long): String {
        val f = RelativeDateTimeFormatter.getInstance(
            ULocale.forLocale(t.locale), null, RelativeDateTimeFormatter.Style.SHORT, DisplayContext.CAPITALIZATION_NONE,
        )
        val minutes = (System.currentTimeMillis() - ms) / 60_000
        val past = RelativeDateTimeFormatter.Direction.LAST
        return when {
            minutes < 1 -> f.format(RelativeDateTimeFormatter.Direction.PLAIN, RelativeDateTimeFormatter.AbsoluteUnit.NOW)
            minutes < 60 -> f.format(minutes.toDouble(), past, RelativeDateTimeFormatter.RelativeUnit.MINUTES)
            minutes < 24 * 60 -> f.format((minutes / 60).toDouble(), past, RelativeDateTimeFormatter.RelativeUnit.HOURS)
            else -> f.format((minutes / 1440).toDouble(), past, RelativeDateTimeFormatter.RelativeUnit.DAYS)
        }
    }

    /** "2 分钟"。 */
    fun lasting(t: Localizer, since: Long): String {
        val minutes = maxOf(1, (System.currentTimeMillis() - since) / 60_000)
        val measure = when {
            minutes < 60 -> Measure(minutes, MeasureUnit.MINUTE)
            minutes < 24 * 60 -> Measure(minutes / 60, MeasureUnit.HOUR)
            else -> Measure(minutes / 1440, MeasureUnit.DAY)
        }
        return MeasureFormat.getInstance(t.locale, MeasureFormat.FormatWidth.SHORT).format(measure)
    }
}
