package ai.ottermind.duolo

import ai.ottermind.duolo.core.Command
import ai.ottermind.duolo.core.OutboxItem
import ai.ottermind.duolo.core.SyncEngine
import ai.ottermind.duolo.core.TaskRow
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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private enum class Filter(val key: String, val group: TaskRow.Group?) {
    All("mobile.filter.all", null),
    Attention("mobile.filter.attention", TaskRow.Group.Attention),
    Active("mobile.filter.active", TaskRow.Group.Active),
    Done("mobile.filter.done", TaskRow.Group.Done),
    ;

    fun matches(row: TaskRow) = group == null || row.group == group
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TaskListScreen(open: (Long) -> Unit, newTask: (String) -> Unit, addComputer: () -> Unit, settings: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val scope = rememberCoroutineScope()
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var menu by rememberSaveable { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }

    val project = model.currentProject
    val all = model.rowsIn(project?.id)
    val q = query.trim()
    val visible = all.filter { row ->
        filter.matches(row) && (q.isEmpty() || listOf(model.title(row.task), row.detail, row.task.branch).any { it.contains(q, ignoreCase = true) })
    }
    // 电脑还没收到的新任务，也要看得到。
    val waiting = model.outbox.filter { item -> q.isEmpty() && filter == Filter.All && (item.command as? Command.CreateTask)?.project == project?.id }

    Box(Modifier.fillMaxSize().background(palette.bg)) {
        Column(Modifier.fillMaxSize().statusBarsPadding()) {
            if (searching) {
                SearchBar(query, { query = it }) {
                    query = ""
                    searching = false
                }
            } else {
                Header(openMenu = { menu = true }, search = { searching = true }, newTask = { newTask("") })
            }
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 2.dp),
                Arrangement.spacedBy(8.dp),
                Alignment.CenterVertically,
            ) { Filter.entries.forEach { f -> Chip(t(f.key), all.count(f::matches), f == filter) { filter = f } } }
            LinkNotice()
            if (model.overview == null) {
                Box(Modifier.fillMaxSize(), Alignment.Center) { CircularProgressIndicator(color = palette.green) }
            } else {
                PullToRefreshBox(
                    refreshing,
                    onRefresh = {
                        refreshing = true
                        scope.launch {
                            model.refresh()
                            refreshing = false
                        }
                    },
                    Modifier.fillMaxSize(),
                ) {
                    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 120.dp)) {
                        items(waiting, { it.id }) { PendingRow(it) }
                        items(visible, { it.id }) { row -> TaskRowView(row) { open(row.id) } }
                        if (visible.isEmpty() && waiting.isEmpty()) item {
                            Column(Modifier.fillMaxWidth().padding(40.dp), Arrangement.spacedBy(6.dp), Alignment.CenterHorizontally) {
                                Label(t(if (all.isEmpty()) "sidebar.noTasks" else "mobile.empty.filtered"), weight = FontWeight.Medium)
                                if (all.isEmpty()) Label(t("mobile.empty.hint"), size = 14.sp, color = palette.muted, align = TextAlign.Center)
                            }
                        }
                    }
                }
            }
        }
        if (project != null) {
            HoldToTalk(Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(16.dp), floating = true) { text, edit ->
                if (edit) newTask(text) else model.submit(Command.CreateTask(project.id, text))
            }
        }
        Menu(menu, { menu = false }, addComputer, settings)
    }
}

@Composable
private fun Header(openMenu: () -> Unit, search: () -> Unit, newTask: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Box {
            IconButton(R.drawable.ic_three_bars, t("mobile.menu.title"), onClick = openMenu)
            if (model.othersNeedYou) Box(Modifier.align(Alignment.TopEnd).padding(10.dp).size(8.dp).clip(CircleShape).background(palette.wait))
        }
        Column(Modifier.weight(1f).padding(start = 4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(R.drawable.ic_repo, size = 16.dp, tint = palette.muted)
                Spacer(Modifier.width(6.dp))
                Label(model.currentProject?.name ?: "Duolo", size = 17.sp, weight = FontWeight.SemiBold, maxLines = 1)
            }
            val name = model.current?.name.orEmpty()
            Row(
                Modifier.semantics(mergeDescendants = true) { contentDescription = "$name, ${t(if (model.computerOnline) "mobile.online" else "mobile.offline")}" },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(if (model.computerOnline) palette.green else palette.dim))
                Spacer(Modifier.width(6.dp))
                Label(name, size = 12.sp, color = palette.muted, maxLines = 1)
            }
        }
        IconButton(R.drawable.ic_search, t("sidebar.search"), onClick = search)
        val enabled = model.currentProject != null
        Box(
            Modifier.size(48.dp).clip(CircleShape).clickable(enabled, role = Role.Button, onClick = newTask)
                .semantics { contentDescription = t("sidebar.newTask") },
            Alignment.Center,
        ) {
            Box(Modifier.size(36.dp).clip(CircleShape).background(palette.greenSoft), Alignment.Center) {
                Icon(R.drawable.ic_plus, tint = palette.greenText)
            }
        }
    }
}

@Composable
private fun SearchBar(query: String, change: (String) -> Unit, cancel: () -> Unit) {
    val t = LocalModel.current.t
    val focus = remember { FocusRequester() }
    val manager = LocalFocusManager.current
    LaunchedEffect(Unit) { focus.requestFocus() }
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.weight(1f).defaultMinSize(minHeight = 48.dp).clip(RoundedCornerShape(24.dp)).background(palette.raised).padding(horizontal = 14.dp),
            Arrangement.spacedBy(8.dp),
            Alignment.CenterVertically,
        ) {
            Icon(R.drawable.ic_search, size = 16.dp, tint = palette.muted)
            BasicTextField(
                query, change, Modifier.weight(1f).focusRequester(focus), singleLine = true, textStyle = LocalTextStyle.current,
                cursorBrush = SolidColor(palette.green), keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions { manager.clearFocus() },
            ) { field ->
                if (query.isEmpty()) Label(t("sidebar.search"), color = palette.muted, maxLines = 1)
                field()
            }
        }
        IconButton(R.drawable.ic_x, t("common.cancel"), onClick = cancel)
    }
}

@Composable
private fun Chip(label: String, count: Int, selected: Boolean, onClick: () -> Unit) = Box(
    Modifier.defaultMinSize(minHeight = 48.dp).clickable(role = Role.Button, onClick = onClick).semantics { this.selected = selected },
    Alignment.Center,
) {
    val color = if (selected) palette.greenText else palette.muted
    Row(
        Modifier.defaultMinSize(minHeight = 32.dp).clip(RoundedCornerShape(16.dp)).background(if (selected) palette.greenSoft else palette.raised)
            .padding(horizontal = 12.dp, vertical = 5.dp),
        Arrangement.spacedBy(6.dp),
        Alignment.CenterVertically,
    ) {
        Label(label, size = 14.sp, weight = FontWeight.Medium, color = color, maxLines = 1)
        Label("$count", size = 14.sp, weight = FontWeight.Medium, color = color)
    }
}

/** 每 30 秒变一次，让"几分钟前"跟着走。 */
@Composable
fun rememberTick(): Long {
    var tick by remember { mutableLongStateOf(0L) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            tick++
        }
    }
    return tick
}

@Composable
private fun TaskRowView(row: TaskRow, onClick: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val style = statusStyle(row.state, t)
    val tick = rememberTick()
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp).semantics(mergeDescendants = true) {},
        Arrangement.spacedBy(14.dp),
        Alignment.CenterVertically,
    ) {
        StatusBadge(style)
        Column(Modifier.weight(1f)) {
            Column(Modifier.padding(vertical = 12.dp), Arrangement.spacedBy(2.dp)) {
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(12.dp)) {
                    Label(model.title(row.task), Modifier.weight(1f).alignByBaseline(), weight = FontWeight.SemiBold, maxLines = 1)
                    Label(style.label, Modifier.alignByBaseline(), size = 13.sp, weight = FontWeight.SemiBold, color = style.color, maxLines = 1)
                }
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(12.dp)) {
                    Label(row.detail, Modifier.weight(1f).alignByBaseline(), size = 14.sp, color = palette.muted, maxLines = 1)
                    val time = remember(tick, row, t) { row.since?.let { Times.lasting(t, it) } ?: Times.ago(t, row.task.updatedAt) }
                    Label(time, Modifier.alignByBaseline(), size = 12.sp, color = palette.dim, maxLines = 1)
                }
            }
            HorizontalDivider(color = palette.line)
        }
    }
}

/** 还没送到电脑的新任务。 */
@Composable
private fun PendingRow(item: OutboxItem) {
    val model = LocalModel.current
    val t = model.t
    val command = item.command as? Command.CreateTask ?: return
    val failed = item.state == OutboxItem.State.failed
    val color = if (failed) palette.bad else palette.gray
    Row(
        Modifier.fillMaxWidth().clickable(failed) { model.retry(item) }.padding(start = 16.dp, top = 8.dp, bottom = 8.dp, end = 4.dp),
        Arrangement.spacedBy(14.dp),
        Alignment.CenterVertically,
    ) {
        Box(Modifier.size(36.dp).clip(RoundedCornerShape(11.dp)).background(color.copy(alpha = 0.13f)), Alignment.Center) {
            Icon(R.drawable.ic_clock, tint = color)
        }
        Column(Modifier.weight(1f)) {
            Label(command.prompt, weight = FontWeight.SemiBold, maxLines = 1)
            Label(
                if (failed) t.error(item.error ?: "internal") else t("mobile.pending"), size = 14.sp,
                color = if (failed) palette.bad else palette.muted,
            )
        }
        if (failed) IconButton(R.drawable.ic_x, t("common.cancel")) { model.discard(item) }
    }
}

/** 连接有问题时一行说明；正常时什么都不显示。`answering`：正有提示要回答，回答不排队，离线时说清楚。 */
@Composable
fun LinkNotice(answering: Boolean = false) {
    val model = LocalModel.current
    val t = model.t
    val link = model.link
    // 刚断开不马上显示，免得一闪一闪。
    var settled by remember { mutableStateOf(false) }
    LaunchedEffect(link) {
        settled = false
        delay(3_000)
        settled = true
    }
    val text = when (link) {
        SyncEngine.Link.Connected(false) -> t(if (answering) "error.computerOffline" else "mobile.banner.offline")
        SyncEngine.Link.Waiting -> if (settled) t("mobile.banner.network") else null
        SyncEngine.Link.Unlinked -> t("mobile.banner.unlinked")
        else -> null
    }
    if (text != null) Label(text, Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), size = 14.sp, color = palette.muted)
}

@Composable
fun VSpace(height: Int) = Spacer(Modifier.height(height.dp))
