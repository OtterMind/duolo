package io.duolo.app

import io.duolo.app.core.AgentOption
import io.duolo.app.core.Localizer
import io.duolo.app.core.TaskRow
import android.icu.text.DisplayContext
import android.icu.text.MeasureFormat
import android.icu.text.RelativeDateTimeFormatter
import android.icu.util.Measure
import android.icu.util.MeasureUnit
import android.icu.util.ULocale
import android.provider.Settings
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
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
/** 状态图标的底色：两个主题一样，上面的白色图标都看得清；和电脑侧边栏同一组。 */
object StateFill {
    val wait = Color(0xFFB07A00)
    val bad = Color(0xFFE0413F)
    val write = Color(0xFF1F74E0)
    val review = Color(0xFF8250DF)
    /** 比微信绿深一档，白色对勾才看得清。 */
    val pass = Color(0xFF05954A)
    val gray = Color(0xFF6E7781)
}

/** [icon] 是圆里的白色图标；审出问题时写问题数（[count]），未启动时两个都没有。[color] 是写成文字时的颜色。 */
data class StatusStyle(val icon: Int?, val count: Int?, val fill: Color, val color: Color, val label: String, val active: Boolean)

@Composable
fun statusStyle(state: TaskRow.State, t: Localizer): StatusStyle {
    val p = palette
    val count = (state as? TaskRow.State.Issues)?.count
    val (icon, fill, color) = when (state) {
        TaskRow.State.NeedsYou -> Triple(R.drawable.ic_ask, StateFill.wait, p.wait)
        is TaskRow.State.Issues -> Triple(null, StateFill.bad, p.bad)
        TaskRow.State.ReviewFailed -> Triple(R.drawable.ic_alert, StateFill.bad, p.bad)
        TaskRow.State.Crashed -> Triple(R.drawable.ic_x, StateFill.bad, p.bad)
        TaskRow.State.Writing -> Triple(R.drawable.ic_pencil, StateFill.write, p.write)
        TaskRow.State.Reviewing -> Triple(R.drawable.ic_eye, StateFill.review, p.review)
        TaskRow.State.Passed -> Triple(R.drawable.ic_check, StateFill.pass, p.greenText)
        TaskRow.State.Reviewed -> Triple(R.drawable.ic_eye, StateFill.gray, p.gray)
        TaskRow.State.Written -> Triple(R.drawable.ic_check, StateFill.gray, p.gray)
        TaskRow.State.Ended -> Triple(R.drawable.ic_dash, StateFill.gray, p.gray)
        TaskRow.State.NotStarted -> Triple(null, StateFill.gray, p.gray)
    }
    // 文案和电脑共用 state.*。
    val label = t("state.${state.code}", "count" to (count ?: 0))
    return StatusStyle(icon, count, fill, color, label, state.group == TaskRow.Group.Active)
}

/**
 * 状态图标：纯色圆底加白色图标（和电脑侧边栏一样）。进行中的圆小一圈，外面一段弧线在转（系统关了动画就不转）；
 * 未启动只有一个虚线圈。读屏念状态名。
 */
@Composable
fun StatusBadge(style: StatusStyle, size: Dp = 32.dp) {
    val dim = palette.dim
    val still = LocalContext.current.let { Settings.Global.getFloat(it.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
    val turn = if (style.active && !still) {
        val angle by rememberInfiniteTransition(label = "turn").animateFloat(0f, 360f, infiniteRepeatable(tween(1600, easing = LinearEasing)), "angle")
        angle
    } else {
        0f
    }
    Box(Modifier.size(size).semantics { contentDescription = style.label }, Alignment.Center) {
        Canvas(Modifier.matchParentSize()) {
            val ring = this.size.minDimension / 16
            when {
                style.icon == null && style.count == null -> drawCircle(
                    dim, radius = this.size.minDimension / 2 - ring * 0.75f,
                    style = Stroke(ring * 0.75f, pathEffect = PathEffect.dashPathEffect(floatArrayOf(ring * 1.5f, ring * 1.6f))),
                )
                style.active -> {
                    val r = this.size.minDimension / 2 - ring / 2
                    drawCircle(style.fill.copy(alpha = 0.28f), r, style = Stroke(ring))
                    drawArc(
                        style.fill, turn - 90f, 90f, false, Offset(ring / 2, ring / 2), Size(r * 2, r * 2),
                        style = Stroke(ring, cap = StrokeCap.Round),
                    )
                    drawCircle(style.fill, this.size.minDimension * 0.39f)
                }
                else -> drawCircle(style.fill)
            }
        }
        val count = style.count
        when {
            count != null -> Text(
                if (count > 9) "9+" else "$count", color = Color.White, fontFamily = MonaSans, fontWeight = FontWeight.SemiBold,
                // 按圆的大小换算，不跟着系统字号变大。
                fontSize = with(LocalDensity.current) { (size * if (count > 9) 0.38f else 0.47f).toSp() },
            )
            style.icon != null -> Icon(style.icon, size = size * if (style.active) 0.4f else 0.5f, tint = Color.White)
        }
    }
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
