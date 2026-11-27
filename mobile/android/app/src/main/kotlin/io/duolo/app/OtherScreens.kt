package io.duolo.app

import io.duolo.app.core.AppError
import io.duolo.app.core.Command
import io.duolo.app.core.Computer
import io.duolo.app.core.Localizer
import io.duolo.app.core.PairingLink
import io.duolo.app.core.SyncEngine
import io.duolo.app.core.TaskRow
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.launch

// ---- 连接电脑 ----

@Composable
fun PairScreen(cancel: (() -> Unit)? = null) {
    val model = LocalModel.current
    val t = model.t
    val context = LocalContext.current
    val scan = rememberLauncherForActivityResult(ScanContract()) { result -> result.contents?.let(model::handlePairing) }
    Column(Modifier.fillMaxSize().background(palette.bg).statusBarsPadding().navigationBarsPadding().padding(horizontal = 24.dp)) {
        if (cancel != null) IconButton(R.drawable.ic_x, t("common.cancel"), Modifier.offset(x = (-12).dp), onClick = cancel)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), Arrangement.Center) {
            Logo()
            VSpace(32)
            Label(t("mobile.pair.title"), Modifier.semantics { heading() }, size = 28.sp, weight = FontWeight.SemiBold)
            VSpace(8)
            Label(t("mobile.pair.body"), color = palette.muted)
        }
        BigButton(t("mobile.pair.scan")) {
            scan.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt("").setBeepEnabled(false).setOrientationLocked(true))
        }
        VSpace(8)
        BigButton(t("mobile.pair.paste"), primary = false) {
            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            model.handlePairing(clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(context)?.toString().orEmpty())
        }
        Label(
            t("mobile.pair.privacy"), Modifier.fillMaxWidth().padding(vertical = 16.dp), size = 13.sp, color = palette.dim, align = TextAlign.Center,
        )
    }
}

/** 标志：实心圆叠一个圆环，代表一写一审。和应用图标一致。 */
@Composable
private fun Logo() = Box {
    Box(Modifier.offset(x = 12.dp).size(38.dp).border(4.4.dp, palette.text, CircleShape))
    Box(Modifier.size(38.dp).clip(CircleShape).background(palette.text))
}

/** 确认要连到哪个服务器：链接可能来自别的应用，不能不问就连。 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairConfirm(link: PairingLink) {
    val model = LocalModel.current
    val t = model.t
    val scope = rememberCoroutineScope()
    var working by remember(link) { mutableStateOf(false) }
    var failure by remember(link) { mutableStateOf<AppError?>(null) }
    ModalBottomSheet({ if (!working) model.pendingPairing = null }, containerColor = palette.bg, contentColor = palette.text) {
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), Arrangement.spacedBy(12.dp)) {
            Label(t("mobile.pair.confirm", "server" to link.serverName), size = 20.sp, weight = FontWeight.SemiBold)
            Label(t("mobile.pair.privacy"), size = 14.sp, color = palette.muted)
            failure?.let { Label(t.error(it.code), size = 14.sp, color = palette.bad) }
            BigButton(t(if (working) "mobile.connecting" else "mobile.pair.connect"), enabled = !working) {
                working = true
                failure = null
                scope.launch {
                    try {
                        model.pair(link)
                        model.pendingPairing = null
                    } catch (e: AppError) {
                        failure = e
                    }
                    working = false
                }
            }
            BigButton(t("common.cancel"), primary = false, enabled = !working) { model.pendingPairing = null }
        }
    }
}

// ---- 新任务 ----

/** 整页只写需求，其他按电脑上的默认值，要改再展开。 */
@Composable
fun NewTaskScreen(initial: String, close: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val overview = model.overview
    var prompt by rememberSaveable { mutableStateOf(initial) }
    var expanded by rememberSaveable { mutableStateOf(false) }
    var project by rememberSaveable { mutableStateOf<String?>(null) }
    var base by rememberSaveable { mutableStateOf<String?>(null) }
    var writer by rememberSaveable { mutableStateOf<String?>(null) }
    var reviewer by rememberSaveable { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { if (initial.isEmpty()) focus.requestFocus() }

    val chosen = overview?.projects?.firstOrNull { it.id == project } ?: model.currentProject
    val writerKind = writer ?: overview?.writer.orEmpty()
    val reviewerKind = reviewer ?: overview?.reviewer.orEmpty()
    val ready = prompt.isNotBlank() && !sending && chosen != null

    Column(Modifier.fillMaxSize().background(palette.bg).statusBarsPadding().imePadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 16.dp, top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(R.drawable.ic_x, t("common.cancel"), onClick = close)
            Spacer(Modifier.weight(1f))
            Box(
                Modifier.defaultMinSize(minHeight = 44.dp).clip(RoundedCornerShape(22.dp)).background(palette.green).clickable(ready, role = Role.Button) {
                    sending = true
                    model.submit(Command.CreateTask(chosen!!.id, prompt.trim(), base = base, writer = writer, reviewer = reviewer)) { ok ->
                        sending = false
                        if (ok) {
                            model.choose(chosen.id)
                            close()
                        }
                    }
                }.alpha(if (ready) 1f else 0.45f).padding(horizontal = 20.dp, vertical = 10.dp),
                Alignment.Center,
            ) { Label(t("mobile.send"), weight = FontWeight.Medium, color = palette.onGreen) }
        }
        Column(Modifier.verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 16.dp)) {
            BasicTextField(
                prompt, { prompt = it }, Modifier.fillMaxWidth().defaultMinSize(minHeight = 120.dp).focusRequester(focus),
                textStyle = LocalTextStyle.current.copy(fontSize = 20.sp, lineHeight = 30.sp), cursorBrush = SolidColor(palette.green),
            ) { field ->
                if (prompt.isEmpty()) Label(t("mobile.new.prompt"), size = 20.sp, color = palette.muted)
                field()
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Row(
                    Modifier.weight(1f).clickable(role = Role.Button) { expanded = !expanded }.defaultMinSize(minHeight = 48.dp),
                    Arrangement.spacedBy(6.dp),
                    Alignment.CenterVertically,
                ) {
                    Label(
                        t(
                            "mobile.new.summary", "project" to chosen?.name.orEmpty(), "writer" to model.agentName(writerKind),
                            "reviewer" to model.agentName(reviewerKind),
                        ),
                        Modifier.weight(1f, fill = false), size = 14.sp, color = palette.muted,
                    )
                    Icon(R.drawable.ic_chevron_down, Modifier.rotate(if (expanded) 180f else 0f), 14.dp, palette.muted)
                }
                HoldToTalk { said, _ -> prompt = if (prompt.isEmpty()) said else "$prompt\n$said" }
            }
            if (expanded) {
                HorizontalDivider(Modifier.padding(vertical = 8.dp), color = palette.line)
                val agents = overview?.agents.orEmpty().map { it.kind to it.name }
                Choice(t("newTask.project"), chosen?.name.orEmpty(), overview?.projects.orEmpty().map { it.id to it.name }) {
                    project = it
                    base = null
                }
                Choice(t("newTask.baseBranch"), base ?: chosen?.branch.orEmpty(), chosen?.branches.orEmpty().map { it to it }) { base = it }
                Choice(t("newTask.writerTitle"), model.agentName(writerKind), agents) { writer = it }
                Choice(t("newTask.reviewerTitle"), model.agentName(reviewerKind), agents) { reviewer = it }
            }
            if (!model.computerOnline) Label(t("mobile.new.queued"), Modifier.padding(top = 8.dp), size = 14.sp, color = palette.muted)
        }
    }
}

/** 一行：左边名字，右边当前的值，点开选。 */
@Composable
private fun Choice(label: String, value: String, items: List<Pair<String, String>>, choose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.fillMaxWidth().clickable(items.isNotEmpty(), role = Role.DropdownList) { open = true }.defaultMinSize(minHeight = 52.dp),
            Arrangement.spacedBy(12.dp),
            Alignment.CenterVertically,
        ) {
            Label(label, Modifier.weight(1f), color = palette.muted)
            Label(value, Modifier.weight(1f), align = TextAlign.End)
            Icon(R.drawable.ic_chevron_down, size = 12.dp, tint = palette.dim)
        }
        Box(Modifier.align(Alignment.BottomEnd)) {
            DropdownMenu(open, { open = false }) {
                items.forEach { (id, name) ->
                    DropdownMenuItem({ Label(name) }, {
                        open = false
                        choose(id)
                    }, trailingIcon = { if (name == value) Icon(R.drawable.ic_check, size = 14.dp, tint = palette.greenText) })
                }
            }
        }
    }
}

// ---- 菜单 ----

/** 左边滑出的菜单：切换仓库和电脑、设置。 */
@Composable
fun Menu(open: Boolean, close: () -> Unit, addComputer: () -> Unit, settings: () -> Unit) {
    BackHandler(open, close)
    AnimatedVisibility(open, enter = fadeIn(), exit = fadeOut()) {
        Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.45f)).clickable(remember { MutableInteractionSource() }, null, onClick = close))
    }
    AnimatedVisibility(open, enter = slideInHorizontally { -it }, exit = slideOutHorizontally { -it }) {
        val model = LocalModel.current
        val t = model.t
        Column(
            Modifier.width(318.dp).fillMaxHeight().background(palette.bg).clickable(remember { MutableInteractionSource() }, null) {}
                .statusBarsPadding().navigationBarsPadding().padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                MenuHeading(t("mobile.menu.projects"))
                model.overview?.projects.orEmpty().forEach { project ->
                    val rows = model.rowsIn(project.id)
                    val waiting = rows.count { it.group == TaskRow.Group.Attention }
                    val active = rows.count { it.group == TaskRow.Group.Active }
                    val note = when {
                        waiting > 0 -> t("sidebar.needsYou", "count" to waiting) to palette.wait
                        active > 0 -> t("mobile.menu.active", "count" to active) to palette.write
                        else -> null
                    }
                    MenuItem(R.drawable.ic_file_directory, project.name, note, project.id == model.currentProject?.id) {
                        model.choose(project.id)
                        close()
                    }
                }
                MenuHeading(t("mobile.menu.computers"))
                model.computers.forEach { computer ->
                    val current = computer.id == model.current?.id
                    val note = if (current) linkLabel(model.link, t) to (if (model.computerOnline) palette.greenText else palette.dim) else null
                    MenuItem(R.drawable.ic_device_desktop, computer.name, note, current && model.computers.size > 1) { model.select(computer) }
                }
                MenuItem(R.drawable.ic_plus, t("mobile.menu.add")) {
                    close()
                    addComputer()
                }
            }
            MenuItem(R.drawable.ic_gear, t("settings.title")) {
                close()
                settings()
            }
        }
    }
}

private fun linkLabel(link: SyncEngine.Link, t: Localizer) = when (link) {
    is SyncEngine.Link.Connected -> t(if (link.computerOnline) "mobile.online" else "mobile.offline")
    SyncEngine.Link.Connecting, SyncEngine.Link.Waiting -> t("mobile.connecting")
    SyncEngine.Link.Unlinked -> t("mobile.offline")
}

@Composable
private fun MenuHeading(text: String) = Label(
    text, Modifier.padding(start = 12.dp, end = 12.dp, top = 20.dp, bottom = 6.dp).semantics { heading() }, size = 12.sp, weight = FontWeight.SemiBold,
    color = palette.dim,
)

@Composable
private fun MenuItem(icon: Int, label: String, note: Pair<String, Color>? = null, selected: Boolean = false, onClick: () -> Unit) = Row(
    Modifier.fillMaxWidth().defaultMinSize(minHeight = 48.dp).clip(RoundedCornerShape(12.dp))
        .background(if (selected) palette.greenSoft else Color.Transparent).clickable(role = Role.Button, onClick = onClick)
        .semantics { this.selected = selected }.padding(horizontal = 12.dp),
    Arrangement.spacedBy(14.dp),
    Alignment.CenterVertically,
) {
    Icon(icon, tint = if (selected) palette.greenText else palette.muted)
    Label(label, Modifier.weight(1f), weight = FontWeight.Medium, maxLines = 1)
    note?.let { (text, color) -> Label(text, size = 13.sp, weight = FontWeight.SemiBold, color = color, maxLines = 1) }
}

// ---- 设置 ----

@Composable
fun SettingsScreen(close: () -> Unit) {
    val model = LocalModel.current
    val t = model.t
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var disconnecting by remember { mutableStateOf<Computer?>(null) }
    var licenses by rememberSaveable { mutableStateOf(false) }
    BackHandler(licenses) { licenses = false }

    Column(Modifier.fillMaxSize().background(palette.bg).statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(R.drawable.ic_chevron_left, t("mobile.back")) { if (licenses) licenses = false else close() }
            Label(
                t(if (licenses) "mobile.settings.licenses" else "settings.title"), Modifier.weight(1f).semantics { heading() },
                weight = FontWeight.SemiBold, align = TextAlign.Center,
            )
            Spacer(Modifier.size(48.dp))
        }
        if (licenses) {
            val text = remember { listOf("LICENSES.md", "OFL.txt").joinToString("\n\n") { f -> context.assets.open(f).use { it.readBytes().decodeToString() } } }
            Text(
                text, Modifier.verticalScroll(rememberScrollState()).padding(20.dp), color = palette.muted, fontFamily = FontFamily.Monospace,
                fontSize = 12.sp, lineHeight = 18.sp,
            )
            return@Column
        }
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 24.dp)) {
            val languages = listOf("auto" to t("sidebar.languageAuto")) + Localizer.locales
            Choice(t("settings.language"), languages.first { it.first == model.language }.second, languages, model::setLanguagePreference)
            val looks = listOf("auto" to t("mobile.settings.system"), "light" to t("mobile.settings.light"), "dark" to t("mobile.settings.dark"))
            Choice(t("mobile.settings.appearance"), looks.first { it.first == model.appearance }.second, looks, model::setAppearancePreference)
            model.current?.let { computer ->
                HorizontalDivider(Modifier.padding(vertical = 12.dp), color = palette.line)
                Label(computer.name, size = 13.sp, color = palette.dim)
                Link(t("mobile.settings.disconnect"), palette.bad) { disconnecting = computer }
            }
            HorizontalDivider(Modifier.padding(vertical = 12.dp), color = palette.line)
            Link(t("mobile.settings.licenses")) { licenses = true }
            Label(t("mobile.settings.version", "version" to BuildConfig.VERSION_NAME), size = 13.sp, color = palette.dim)
        }
    }
    disconnecting?.let { computer ->
        AlertDialog(
            onDismissRequest = { disconnecting = null },
            title = { Label(t("mobile.settings.disconnectTitle", "computer" to computer.name), size = 18.sp, weight = FontWeight.SemiBold) },
            text = { Label(t("mobile.settings.disconnectBody"), size = 14.sp, color = palette.muted) },
            confirmButton = {
                TextButton({
                    disconnecting = null
                    scope.launch {
                        model.disconnect(computer)
                        close()
                    }
                }) { Label(t("mobile.settings.disconnectAction"), color = palette.bad, weight = FontWeight.Medium) }
            },
            dismissButton = { TextButton({ disconnecting = null }) { Label(t("common.cancel"), weight = FontWeight.Medium) } },
        )
    }
}

@Composable
private fun Link(text: String, color: Color = palette.text, onClick: () -> Unit) = Box(
    Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick).defaultMinSize(minHeight = 52.dp).semantics { contentDescription = text },
    Alignment.CenterStart,
) { Label(text, color = color) }
