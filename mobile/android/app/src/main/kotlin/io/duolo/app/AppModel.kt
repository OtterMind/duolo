package io.duolo.app

import io.duolo.app.core.AppError
import io.duolo.app.core.VoiceHints
import io.duolo.app.core.Command
import io.duolo.app.core.Computer
import io.duolo.app.core.LocalStore
import io.duolo.app.core.Localizer
import io.duolo.app.core.OkHttpTransport
import io.duolo.app.core.OutboxItem
import io.duolo.app.core.Overview
import io.duolo.app.core.Pairing
import io.duolo.app.core.PairingLink
import io.duolo.app.core.ProjectOverview
import io.duolo.app.core.SyncEngine
import io.duolo.app.core.TaskOverview
import io.duolo.app.core.TaskRow
import io.duolo.app.core.Timeline
import io.duolo.app.core.defaultHttp
import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.edit
import androidx.core.os.ConfigurationCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class DuoloApplication : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        model = AppModel(this)
    }
}

/** 界面用的全部状态。数据都从 `LocalStore` 读，`SyncEngine` 说变了就重新读一遍，界面不直接碰网络。只在主线程上改。 */
class AppModel(private val app: Application) : DefaultLifecycleObserver {
    data class OpenedTask(val id: Long, val timeline: Timeline, val review: String?)

    // ---- 数据 ----

    var computers by mutableStateOf<List<Computer>>(emptyList())
        private set
    var current by mutableStateOf<Computer?>(null)
        private set
    var overview by mutableStateOf<Overview?>(null)
        private set
    var rows by mutableStateOf<List<TaskRow>>(emptyList())
        private set
    var link by mutableStateOf<SyncEngine.Link>(SyncEngine.Link.Connecting)
        private set
    var outbox by mutableStateOf<List<OutboxItem>>(emptyList())
        private set
    var opened by mutableStateOf<OpenedTask?>(null)
        private set

    /** 要告诉用户的错误，显示几秒。 */
    var problem by mutableStateOf<AppError?>(null)

    /** 等用户确认的配对链接。 */
    var pendingPairing by mutableStateOf<PairingLink?>(null)

    // ---- 设置 ----

    private val prefs = app.getSharedPreferences("settings", Context.MODE_PRIVATE)

    /** 界面语言：`auto` 跟随系统，和桌面端一样。 */
    var language by mutableStateOf(prefs.getString("language", "auto") ?: "auto")
        private set

    /** `auto`、`light`、`dark`。 */
    var appearance by mutableStateOf(prefs.getString("appearance", "auto") ?: "auto")
        private set
    var t by mutableStateOf(localizer())
        private set
    private var project by mutableStateOf<String?>(null)


    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val store = LocalStore(AndroidDb.open(app))
    private val secrets = KeystoreSecrets(app)
    private val http = defaultHttp()

    /** 正在进行的语音输入，面板由最上层的界面画。 */
    val voice = VoiceSession(app, http)
    private val transport = OkHttpTransport(http)
    private val pairing = Pairing(store, secrets, http)
    private var engine: SyncEngine? = null
    private var listener: Job? = null
    private var background: Job? = null

    init {
        computers = store.computers()
        select(computers.firstOrNull { it.id == prefs.getString("computer", null) } ?: computers.firstOrNull())
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
        app.getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                scope.launch { engine?.nudge() }
            }
        })
    }

    fun setLanguagePreference(code: String) {
        language = code
        prefs.edit { putString("language", code) }
        t = localizer()
    }

    fun setAppearancePreference(value: String) {
        appearance = value
        prefs.edit { putString("appearance", value) }
    }

    /** 系统语言变了（应用重新建界面时调用）。 */
    fun systemLocaleChanged() {
        if (language == "auto") t = localizer()
    }

    private fun localizer(): Localizer {
        val tags = if (language == "auto") {
            val locales = ConfigurationCompat.getLocales(app.resources.configuration)
            (0 until locales.size()).mapNotNull { locales[it]?.toLanguageTag() }
        } else {
            listOf(language)
        }
        return Localizer(Localizer.match(tags)) { app.assets.open("locales/$it") }
    }

    // ---- 电脑 ----

    fun select(computer: Computer?) {
        if (computer?.id == current?.id && engine != null) return
        listener?.cancel()
        engine?.let { old -> scope.launch { old.stop() } }
        engine = null
        current = computer
        opened = null
        link = SyncEngine.Link.Connecting
        prefs.edit { putString("computer", computer?.id) }
        project = computer?.let { prefs.getString("project.${it.id}", null) }
        reloadOverview()
        reloadOutbox()
        if (computer == null) return
        val secret = secrets.load(computer.id)
        val next = secret?.let { runCatching { SyncEngine(computer, it, store, transport) }.getOrNull() }
        if (next == null) {
            // 密钥库里没有（例如换了手机）：要重新配对。
            link = SyncEngine.Link.Unlinked
            return
        }
        engine = next
        listener = scope.launch {
            launch {
                next.link.collect {
                    link = it
                    computers = store.computers()
                    current = computers.firstOrNull { c -> c.id == computer.id } ?: current
                }
            }
            launch { next.changes.collect(::apply) }
            next.start()
        }
    }

    private fun apply(change: SyncEngine.Change) {
        when (change) {
            SyncEngine.Change.Overview -> reloadOverview()
            is SyncEngine.Change.Task -> if (opened?.id == change.id) reloadTask()
            SyncEngine.Change.Outbox -> reloadOutbox()
            is SyncEngine.Change.Failure -> problem = change.error
        }
    }

    suspend fun pair(link: PairingLink) {
        val name = "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}"
        val computer = withContext(Dispatchers.IO) { pairing.connect(link, name, "android", System.currentTimeMillis()) }
        computers = store.computers()
        // 重新配对同一台时也要换上新的密钥。
        engine?.stop()
        engine = null
        current = null
        select(computer)
    }

    suspend fun disconnect(computer: Computer) {
        if (computer.id == current?.id) engine?.stop()
        try {
            withContext(Dispatchers.IO) { pairing.disconnect(computer) }
        } catch (e: AppError) {
            problem = e
        }
        computers = store.computers()
        if (computer.id == current?.id) {
            engine = null
            current = null
            select(computers.firstOrNull())
        }
    }

    // ---- 前后台 ----

    override fun onStart(owner: LifecycleOwner) {
        background?.cancel()
        scope.launch {
            engine?.start()
            engine?.nudge()
        }
    }

    override fun onStop(owner: LifecycleOwner) {
        // 留一点时间把刚发的送出去，再断开。
        background = scope.launch {
            delay(5_000)
            engine?.stop()
        }
    }

    // ---- 任务 ----

    private fun reloadOverview() {
        overview = current?.let { store.overview(it.id) }
        rows = TaskRow.rows(overview?.tasks.orEmpty())
        // 没选过（或者选的没了）就定在现在这个，不能看着看着自己跳到别的仓库。
        currentProject?.id?.let { if (it != project) choose(it) }
        if (opened != null) reloadTask()
    }

    private fun reloadOutbox() {
        outbox = current?.let { store.outbox(it.id) }.orEmpty()
    }

    /** 正在看的仓库：选过的，不然是最近有动静的任务所在的。 */
    val currentProject: ProjectOverview?
        get() {
            val projects = overview?.projects.orEmpty()
            return projects.firstOrNull { it.id == project }
                ?: overview?.tasks?.maxByOrNull { it.updatedAt }?.let { recent -> projects.firstOrNull { it.id == recent.project } }
                ?: projects.firstOrNull()
        }

    fun choose(project: String) {
        this.project = project
        current?.let { prefs.edit { putString("project.${it.id}", project) } }
    }

    fun rowsIn(project: String?): List<TaskRow> = rows.filter { it.task.project == project }

    /** 别的仓库里有任务在等。 */
    val othersNeedYou: Boolean get() = rows.any { it.group == TaskRow.Group.Attention && it.task.project != currentProject?.id }

    fun row(id: Long): TaskRow? = rows.firstOrNull { it.id == id }

    fun title(task: TaskOverview): String = if (task.untitled) t("task.untitled") else task.title

    fun agentName(kind: String): String =
        overview?.agents?.firstOrNull { it.kind == kind }?.name ?: kind.replaceFirstChar { it.uppercase() }

    fun open(id: Long) {
        opened = OpenedTask(id, Timeline(), null)
        reloadTask()
        scope.launch { engine?.open(id) }
    }

    fun leave(id: Long) {
        if (opened?.id != id) return
        opened = null
        scope.launch { engine?.open(null) }
    }

    private fun reloadTask() {
        val computer = current ?: return
        val id = opened?.id ?: return
        scope.launch {
            val (timeline, review) = withContext(Dispatchers.IO) { Timeline(store.events(computer.id, id)) to store.review(computer.id, id) }
            if (opened?.id == id) opened = OpenedTask(id, timeline, review)
        }
    }

    suspend fun refresh() {
        engine?.refresh()
        delay(600)
    }

    /** 让电脑做一件事，出错时告诉用户。`done` 的参数是有没有成功交出去。 */
    fun submit(command: Command, done: (Boolean) -> Unit = {}) {
        val engine = engine
        if (engine == null) {
            problem = AppError.unauthorized
            done(false)
            return
        }
        scope.launch {
            val ok = try {
                engine.submit(command)
                true
            } catch (e: AppError) {
                problem = e
                false
            }
            done(ok)
        }
    }

    fun retry(item: OutboxItem) {
        scope.launch {
            try {
                engine?.retry(item.id)
            } catch (e: AppError) {
                problem = e
            }
        }
    }

    fun discard(item: OutboxItem) {
        scope.launch { engine?.discard(item.id) }
    }

    /** 给这个任务的、还没送到或没成功的。 */
    fun pending(task: Long): List<OutboxItem> = outbox.filter { it.command.task == task }

    val computerOnline: Boolean get() = link == SyncEngine.Link.Connected(true)

    /** 语音识别用当前电脑配对的服务器和登录凭据；没配对时为空。 */
    fun voiceRoute(): Pair<String, String>? {
        val computer = current ?: return null
        val secret = runCatching { secrets.load(computer.id) }.getOrNull() ?: return null
        return computer.server to secret.token
    }

    /** 给识别服务的提示：打开着的任务的文件名、代码名字和最近的对话；没打开任务时只有项目名。 */
    fun voiceHints(): VoiceHints {
        val computer = current
        val task = opened?.id?.let { id -> rows.firstOrNull { it.id == id }?.task }
        if (computer == null || task == null) return VoiceHints.of(currentProject?.name, null, null, emptyList())
        val events = runCatching { store.events(computer.id, task.id) }.getOrDefault(emptyList())
        val name = overview?.projects?.firstOrNull { it.id == task.project }?.name ?: currentProject?.name
        return VoiceHints.of(name, task.title.takeUnless { task.untitled }, task.branch, events)
    }

    /** 错误给用户看的说明。 */
    fun message(problem: AppError): String = when (problem.code) {
        "voicePermission" -> t("mobile.voice.permission")
        else -> t.error(problem.code)
    }

    /** 扫到、粘贴的内容，或者点开的链接。 */
    fun handlePairing(text: String) {
        val link = PairingLink.parse(text)
        if (link != null) pendingPairing = link else problem = AppError.badPairingLink
    }
}
