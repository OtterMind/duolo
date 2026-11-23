package ai.ottermind.duolo

import ai.ottermind.duolo.core.Command
import ai.ottermind.duolo.core.Localizer
import ai.ottermind.duolo.core.OutboxItem
import ai.ottermind.duolo.core.PromptOverview
import ai.ottermind.duolo.core.TaskOverview
import ai.ottermind.duolo.core.TaskRow
import ai.ottermind.duolo.core.Timeline
import ai.ottermind.duolo.core.reviewBody
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material3.Text

@Composable
fun TaskScreen(id: Long, back: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val row = model.row(id)
    val timeline = model.opened?.takeIf { it.id == id }?.timeline ?: Timeline()
    var draft by rememberSaveable { mutableStateOf("") }
    var steps by rememberSaveable { mutableStateOf(false) }
    var confirmClose by rememberSaveable { mutableStateOf(false) }

    DisposableEffect(id) {
        model.open(id)
        onDispose { model.leave(id) }
    }

    Column(Modifier.fillMaxSize().background(palette.bg).statusBarsPadding().imePadding()) {
        TaskHeader(row, back, showSteps = { steps = true }, close = { confirmClose = true })
        if (row == null) {
            Box(Modifier.fillMaxSize(), Alignment.Center) { Label(t("error.taskNotFound"), color = palette.muted) }
        } else {
            Conversation(row, timeline, Modifier.weight(1f)) { steps = true }
            LinkNotice(answering = row.state == TaskRow.State.NeedsYou)
            val prompt = row.task.prompt
            if (row.state == TaskRow.State.NeedsYou && prompt != null) {
                PromptPanel(row.task, prompt)
            } else {
                Column(Modifier.navigationBarsPadding()) {
                    if (row.state == TaskRow.State.NeedsYou) {
                        Row(Modifier.padding(horizontal = 24.dp, vertical = 6.dp), Arrangement.spacedBy(10.dp), Alignment.CenterVertically) {
                            StatusBadge(statusStyle(row.state, t), 28.dp)
                            Label(t("mobile.prompt.unreadable"), size = 14.sp, color = palette.wait)
                        }
                    }
                    Activity(row, timeline) { steps = true }
                    Composer(draft, { draft = it }, Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 8.dp)) {
                        model.submit(Command.SendMessage(id, it))
                    }
                }
            }
        }
    }
    if (steps) StepsSheet(row?.task, timeline) { steps = false }
    if (confirmClose && row != null) {
        AlertDialog(
            onDismissRequest = { confirmClose = false },
            title = { Label(t("confirm.closeTitle", "title" to model.title(row.task)), size = 18.sp, weight = FontWeight.SemiBold) },
            text = { Label(t("confirm.closeBody", "branch" to row.task.branch), size = 14.sp, color = palette.muted) },
            confirmButton = {
                TextButton({
                    confirmClose = false
                    model.submit(Command.CloseTask(id)) { ok -> if (ok) back() }
                }) { Label(t("confirm.closeAction"), color = palette.bad, weight = FontWeight.Medium) }
            },
            dismissButton = { TextButton({ confirmClose = false }) { Label(t("common.cancel"), weight = FontWeight.Medium) } },
        )
    }
}

@Composable
private fun TaskHeader(row: TaskRow?, back: () -> Unit, showSteps: () -> Unit, close: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val context = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(R.drawable.ic_chevron_left, t("mobile.back"), onClick = back)
        Label(
            row?.let { model.title(it.task) }.orEmpty(), Modifier.weight(1f).semantics { heading() }, weight = FontWeight.SemiBold, maxLines = 1,
            align = TextAlign.Center,
        )
        Box {
            IconButton(R.drawable.ic_kebab_horizontal, t("common.more"), enabled = row != null) { menu = true }
            DropdownMenu(menu, { menu = false }) {
                @Composable
                fun item(text: String, color: Color = palette.text, action: () -> Unit) = DropdownMenuItem({ Label(text, color = color) }, {
                    menu = false
                    action()
                })
                item(t("mobile.steps.title"), action = showSteps)
                if (row != null && row.state != TaskRow.State.Reviewing) {
                    item(t(if (row.task.review == null) "review.start" else "review.rerun")) { model.submit(Command.StartReview(row.id)) }
                }
                item(t("sidebar.copyBranch")) { row?.let { copy(context, it.task.branch) } }
                item(t("confirm.closeAction"), palette.bad, close)
            }
        }
    }
}

private fun copy(context: Context, text: String) =
    (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(text, text))

@Composable
private fun Conversation(row: TaskRow, timeline: Timeline, modifier: Modifier, showSteps: () -> Unit) {
    val model = LocalModel.current
    val pending = model.pending(row.id)
    val review = model.opened?.takeIf { it.id == row.id }?.review?.takeIf { row.state is TaskRow.State.Issues }
    // 最近一次审阅的全文在下面，时间线里不重复。
    val expanded = if (review != null) timeline.items.lastOrNull { it is Timeline.Item.Review }?.seq else null
    val list = rememberLazyListState()
    val count = timeline.items.size + (if (review != null) 1 else 0) + pending.size
    // 有新内容就滚到底；往上翻着看的时候不动。
    LaunchedEffect(count) {
        val last = list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
        if (count > 0 && (last == 0 || last >= list.layoutInfo.totalItemsCount - 3)) list.scrollToItem(count - 1, Int.MAX_VALUE / 2)
    }
    LazyColumn(modifier.fillMaxWidth(), list, PaddingValues(horizontal = 24.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if (timeline.items.isEmpty() && row.task.seq > 0) item {
            Box(Modifier.fillMaxWidth().padding(40.dp), Alignment.Center) { CircularProgressIndicator(color = palette.green) }
        }
        items(timeline.items, { it.seq }) { item ->
            when (item) {
                is Timeline.Item.User -> Bubble(item.text)
                is Timeline.Item.Agent -> Markdown(item.text)
                is Timeline.Item.Steps -> StepsLine(item.steps, showSteps)
                is Timeline.Item.Review -> if (item.seq != expanded) ReviewLine(item)
            }
        }
        if (review != null) item("review") { ReviewSection(row, review) }
        items(pending, { it.id }) { PendingBubble(it) }
    }
}

@Composable
private fun StepsLine(steps: List<Timeline.Step>, onClick: () -> Unit) {
    val t = LocalModel.current.t
    val n = Timeline.Counts.of(steps)
    val text = listOfNotNull(
        n.read.takeIf { it > 0 }?.let { t("mobile.steps.read", "count" to it) },
        n.edit.takeIf { it > 0 }?.let { t("mobile.steps.edit", "count" to it) },
        n.run.takeIf { it > 0 }?.let { t("mobile.steps.run", "count" to it) },
        n.other.takeIf { it > 0 }?.let { t("mobile.steps.other", "count" to it) },
    ).joinToString(" · ")
    Row(
        Modifier.clickable(role = Role.Button, onClick = onClick).defaultMinSize(minHeight = 40.dp),
        Arrangement.spacedBy(4.dp),
        Alignment.CenterVertically,
    ) {
        Label(text, size = 14.sp, color = palette.muted)
        Icon(R.drawable.ic_chevron_right, size = 14.dp, tint = palette.muted)
    }
}

/** 最近一次审阅的全文，以及交回修改。 */
@Composable
private fun ReviewSection(row: TaskRow, output: String) {
    val model = LocalModel.current
    val t = model.t
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        HorizontalDivider(color = palette.line)
        Row(Modifier.padding(top = 8.dp), Arrangement.spacedBy(10.dp), Alignment.CenterVertically) {
            Icon(R.drawable.ic_issue_opened, tint = palette.bad)
            Label(t("mobile.review.by", "agent" to row.task.reviewer.name), weight = FontWeight.SemiBold)
        }
        Markdown(reviewBody(output))
        BigButton(t("review.sendToWriter", "agent" to row.task.writer.name), enabled = model.pending(row.id).isEmpty()) {
            val text = t("review.handoff", "agent" to row.task.reviewer.name) + "\n\n" + output
            model.submit(Command.SendMessage(row.id, text))
        }
    }
}

@Composable
private fun ReviewLine(item: Timeline.Item.Review) {
    val model = LocalModel.current
    val state = when {
        !item.ok -> TaskRow.State.ReviewFailed
        item.verdict == "issues" -> TaskRow.State.Issues(item.issues)
        item.verdict == "pass" -> TaskRow.State.Passed
        else -> TaskRow.State.Reviewed
    }
    val style = statusStyle(state, model.t)
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(style.icon, size = 16.dp, tint = style.color)
        Label("${model.agentName(item.reviewer)} · ${style.label}", size = 14.sp, weight = FontWeight.Medium, color = style.color)
    }
}

@Composable
private fun Bubble(text: String) = Box(Modifier.fillMaxWidth().padding(start = 48.dp), Alignment.CenterEnd) {
    SelectionContainer {
        Label(text, Modifier.clip(RoundedCornerShape(20.dp)).background(palette.bubble).padding(horizontal = 16.dp, vertical = 10.dp))
    }
}

/** 还没送到、或没成功的话。 */
@Composable
private fun PendingBubble(item: OutboxItem) {
    val model = LocalModel.current
    val t = model.t
    val failed = item.state == OutboxItem.State.failed
    val outline = if (failed) palette.bad else palette.dim
    Column(Modifier.fillMaxWidth().padding(start = 48.dp), Arrangement.spacedBy(6.dp), Alignment.End) {
        (item.command as? Command.SendMessage)?.let {
            val stroke = Stroke(3f, pathEffect = PathEffect.dashPathEffect(floatArrayOf(12f, 9f)))
            Label(
                it.text,
                Modifier.drawBehind { drawRoundRect(outline, style = stroke, cornerRadius = CornerRadius(20.dp.toPx())) }
                    .padding(horizontal = 16.dp, vertical = 10.dp),
                color = palette.muted,
            )
        }
        Label(
            if (failed) "${t("mobile.notDelivered")} · ${t.error(item.error ?: "internal")}" else t("mobile.pending"),
            size = 13.sp, color = if (failed) palette.bad else palette.muted, align = TextAlign.End,
        )
        if (failed) Row {
            TextButton({ model.discard(item) }) { Label(t("common.cancel"), size = 14.sp, weight = FontWeight.Medium) }
            TextButton({ model.retry(item) }) { Label(t("mobile.retry"), size = 14.sp, weight = FontWeight.Medium, color = palette.greenText) }
        }
    }
}

/** 此刻在做什么，右边停止。 */
@Composable
private fun Activity(row: TaskRow, timeline: Timeline, showSteps: () -> Unit) {
    if (row.group != TaskRow.Group.Active) return
    val model = LocalModel.current
    val t = model.t
    val tick = rememberTick()
    val step = timeline.activity
    val text = when {
        row.state == TaskRow.State.Reviewing -> "${t("mobile.state.reviewing")} · ${row.task.reviewer.name}"
        step == null || step.target.isEmpty() -> t("mobile.working", "agent" to row.task.writer.name)
        else -> "${stepLabel(step.kind, t)} ${step.target}"
    }
    Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.weight(1f).clickable(role = Role.Button, onClick = showSteps).defaultMinSize(minHeight = 48.dp),
            Arrangement.spacedBy(10.dp),
            Alignment.CenterVertically,
        ) {
            StatusBadge(statusStyle(row.state, t), 28.dp)
            Label(text, Modifier.weight(1f), size = 14.sp, color = palette.muted, maxLines = 1)
            row.since?.let { since -> Label(remember(tick, since, t) { Times.lasting(t, since) }, size = 14.sp, color = palette.muted, maxLines = 1) }
        }
        if (row.state == TaskRow.State.Writing) {
            IconButton(R.drawable.ic_square_fill, t("mobile.stop"), tint = palette.muted, enabled = model.computerOnline) {
                model.submit(Command.Interrupt(row.id))
            }
        }
    }
}

/** 输入框：有字时是发送，没字时按住说话。 */
@Composable
private fun Composer(text: String, change: (String) -> Unit, modifier: Modifier, send: (String) -> Unit) {
    val t = LocalModel.current.t
    val focus = remember { FocusRequester() }
    Row(
        modifier.fillMaxWidth().clip(RoundedCornerShape(26.dp)).background(palette.raised).padding(start = 20.dp, end = 2.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        BasicTextField(
            text, change, Modifier.weight(1f).padding(vertical = 14.dp).focusRequester(focus), maxLines = 6, textStyle = LocalTextStyle.current,
            cursorBrush = SolidColor(palette.green),
        ) { field ->
            if (text.isEmpty()) Label(t("mobile.composer"), color = palette.muted, maxLines = 1)
            field()
        }
        Box(Modifier.size(48.dp, 52.dp), Alignment.Center) {
            if (text.isBlank()) {
                HoldToTalk { said, edit ->
                    if (edit) {
                        change(said)
                        focus.requestFocus()
                    } else {
                        send(said)
                    }
                }
            } else {
                Box(
                    Modifier.size(48.dp).clip(CircleShape).clickable(role = Role.Button) {
                        send(text.trim())
                        change("")
                    }.semantics { contentDescription = t("mobile.send") },
                    Alignment.Center,
                ) {
                    Box(Modifier.size(36.dp).clip(CircleShape).background(palette.green), Alignment.Center) {
                        Icon(R.drawable.ic_arrow_up, tint = palette.onGreen)
                    }
                }
            }
        }
    }
}

/** Agent 在等你选：选项代替输入框，来自 Agent，原样显示。 */
@Composable
private fun PromptPanel(task: TaskOverview, prompt: PromptOverview) {
    val model = LocalModel.current
    val t = model.t
    var answering by remember(prompt.seq) { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp)).background(palette.raised).navigationBarsPadding()
            .padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 12.dp),
        Arrangement.spacedBy(16.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            StatusBadge(statusStyle(TaskRow.State.NeedsYou, t), 28.dp)
            Label(t("mobile.prompt.title", "agent" to task.writer.name), size = 14.sp, weight = FontWeight.Medium, color = palette.wait)
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            if (prompt.detail.isNotEmpty()) {
                SelectionContainer(Modifier.heightIn(max = 180.dp).verticalScroll(rememberScrollState())) {
                    Text(prompt.detail, color = palette.text, fontFamily = FontFamily.Monospace, fontSize = 15.sp, lineHeight = 22.sp)
                }
            }
            val plain = prompt.detail.isEmpty()
            Label(prompt.question, size = if (plain) 16.sp else 14.sp, color = if (plain) palette.text else palette.muted)
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            prompt.options.forEachIndexed { i, option ->
                BigButton(option, primary = i == 0, enabled = !answering && model.computerOnline) {
                    answering = true
                    model.submit(Command.Answer(task.id, i, prompt.seq)) { answering = false }
                }
            }
        }
    }
}

fun stepLabel(kind: Timeline.StepKind, t: Localizer): String = when (kind) {
    Timeline.StepKind.Read -> t("mobile.step.read")
    Timeline.StepKind.Edit -> t("mobile.step.edit")
    Timeline.StepKind.Run -> t("mobile.step.run")
    Timeline.StepKind.Search -> t("mobile.step.search")
    Timeline.StepKind.Other -> t("mobile.step.other")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun StepsSheet(task: TaskOverview?, timeline: Timeline, close: () -> Unit) {
    val t = LocalModel.current.t
    ModalBottomSheet(close, containerColor = palette.bg, contentColor = palette.text) {
        LazyColumn(contentPadding = PaddingValues(start = 24.dp, end = 24.dp, bottom = 32.dp)) {
            item {
                Label(t("mobile.steps.title"), Modifier.padding(bottom = 12.dp).semantics { heading() }, size = 18.sp, weight = FontWeight.SemiBold)
                (timeline.diff ?: task?.diff)?.let { diff ->
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Label(t("mobile.steps.files", "count" to diff.files))
                        Label("+${diff.insertions}", color = palette.greenText)
                        Label("−${diff.deletions}", color = palette.bad)
                    }
                }
                task?.branch?.takeIf { it.isNotEmpty() }?.let {
                    SelectionContainer { Text(it, color = palette.muted, fontFamily = FontFamily.Monospace, fontSize = 13.sp) }
                }
                VSpace(12)
            }
            items(timeline.steps, { it.seq }) { step ->
                Row(
                    Modifier.fillMaxWidth().defaultMinSize(minHeight = 40.dp).semantics(mergeDescendants = true) {},
                    Arrangement.spacedBy(12.dp),
                    Alignment.CenterVertically,
                ) {
                    Label(stepLabel(step.kind, t), Modifier.width(72.dp), size = 14.sp, color = palette.muted, maxLines = 1)
                    Text(
                        if (step.target.isEmpty() || step.target.startsWith("***")) step.tool else step.target,
                        color = palette.text, fontFamily = FontFamily.Monospace, fontSize = 14.sp, maxLines = 2,
                    )
                }
            }
        }
    }
}

private sealed interface Block {
    data class Paragraph(val text: String) : Block
    data class Heading(val text: String) : Block
    data class Item(val text: String, val marker: String) : Block
    data class Code(val text: String) : Block
}

private val headingLine = Regex("""#{1,6}\s+(.*)""")
private val itemLine = Regex("""([-*+]|\d+[.)])\s+(.*)""")
private val inlineMark = Regex("""\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)]\([^)]*\)""")

private fun parseMarkdown(markdown: String): List<Block> {
    val blocks = mutableListOf<Block>()
    val paragraph = mutableListOf<String>()
    var code: MutableList<String>? = null
    fun flush() {
        if (paragraph.isNotEmpty()) blocks += Block.Paragraph(paragraph.joinToString("\n"))
        paragraph.clear()
    }
    for (line in markdown.lines()) {
        val trimmed = line.trim()
        val lines = code
        when {
            trimmed.startsWith("```") -> if (lines != null) {
                blocks += Block.Code(lines.joinToString("\n"))
                code = null
            } else {
                flush()
                code = mutableListOf()
            }
            lines != null -> lines += line
            trimmed.isEmpty() -> flush()
            headingLine.matches(trimmed) -> {
                flush()
                blocks += Block.Heading(headingLine.matchEntire(trimmed)!!.groupValues[1])
            }
            itemLine.matches(trimmed) -> {
                flush()
                val (marker, text) = itemLine.matchEntire(trimmed)!!.destructured
                blocks += Block.Item(text, if (marker[0].isDigit()) marker else "•")
            }
            else -> paragraph += trimmed
        }
    }
    code?.let { blocks += Block.Code(it.joinToString("\n")) }
    flush()
    return blocks
}

private fun inline(text: String): AnnotatedString = buildAnnotatedString {
    var at = 0
    for (m in inlineMark.findAll(text)) {
        append(text.substring(at, m.range.first))
        val (bold, code, link) = m.destructured
        when {
            bold.isNotEmpty() -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(inline(bold)) }
            code.isNotEmpty() -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, fontSize = 14.sp)) { append(code) }
            else -> append(link)
        }
        at = m.range.last + 1
    }
    append(text.substring(at))
}

/** Agent 的话：段落、标题、列表、代码块。只做常见的格式，其余原样显示。 */
@Composable
fun Markdown(markdown: String) {
    val blocks = remember(markdown) { parseMarkdown(markdown) }
    SelectionContainer {
        Column(Modifier.fillMaxWidth(), Arrangement.spacedBy(10.dp)) {
            for (block in blocks) when (block) {
                is Block.Paragraph -> Text(inline(block.text), color = palette.text, lineHeight = 26.sp)
                is Block.Heading -> Text(inline(block.text), color = palette.text, fontWeight = FontWeight.SemiBold)
                is Block.Item -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Label(block.marker, Modifier.widthIn(min = 10.dp), color = palette.muted)
                    Text(inline(block.text), color = palette.text, lineHeight = 26.sp)
                }
                is Block.Code -> Box(Modifier.clip(RoundedCornerShape(10.dp)).background(palette.raised).horizontalScroll(rememberScrollState())) {
                    Text(block.text, Modifier.padding(12.dp), color = palette.text, fontFamily = FontFamily.Monospace, fontSize = 13.sp, lineHeight = 19.sp, softWrap = false)
                }
            }
        }
    }
}
