package io.duolo.app

import io.duolo.app.core.AppError
import io.duolo.app.core.PairingLink
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.lifecycleScope
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val model get() = (application as DuoloApplication).model

    /** 调试时直接打开的界面，见 [handle]。 */
    private var debugRoute by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model.systemLocaleChanged()
        if (savedInstanceState == null) handle(intent)
        setContent {
            val dark = when (model.appearance) {
                "light" -> false
                "dark" -> true
                else -> isSystemInDarkTheme()
            }
            LaunchedEffect(dark) {
                val transparent = android.graphics.Color.TRANSPARENT
                val style = if (dark) SystemBarStyle.dark(transparent) else SystemBarStyle.light(transparent, transparent)
                enableEdgeToEdge(style, style)
            }
            CompositionLocalProvider(LocalModel provides model) { DuoloTheme(model.appearance) { Root(debugRoute) } }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent) {
        // 链接可能来自别的应用：先问用户，不能直接连。
        intent.data?.takeIf { it.scheme == "duolo" }?.let { model.handlePairing(it.toString()) }
        if (BuildConfig.DEBUG) {
            // 自动化检查用：-e duoloPair <链接> 直接配对，-e duoloRoute task/1 打开某一页。
            intent.getStringExtra("duoloPair")?.let(PairingLink::parse)?.let { link ->
                if (model.computers.isEmpty()) lifecycleScope.launch { runCatching { model.pair(link) }.onFailure { model.problem = it as? AppError } }
            }
            debugRoute = intent.getStringExtra("duoloRoute")
        }
    }
}

@Composable
private fun Root(debugRoute: String?) {
    val model = LocalModel.current
    Box(Modifier.fillMaxSize().background(palette.bg)) {
        if (model.computers.isEmpty()) PairScreen() else MainScreens(debugRoute)
        VoiceOverlay()
        ProblemBanner()
        model.pendingPairing?.let { PairConfirm(it) }
    }
}

@Composable
private fun MainScreens(debugRoute: String?) {
    val model = LocalModel.current
    val nav = rememberNavController()
    /** 说完先改再发的话，带到新任务页。 */
    var draft by remember { mutableStateOf("") }
    LaunchedEffect(debugRoute, model.rows.isEmpty()) { if (debugRoute != null && model.rows.isNotEmpty()) nav.navigate(debugRoute) { launchSingleTop = true } }
    NavHost(nav, startDestination = "list") {
        composable("list") {
            TaskListScreen(
                open = { nav.navigate("task/$it") },
                newTask = {
                    draft = it
                    nav.navigate("new")
                },
                addComputer = { nav.navigate("pair") },
                settings = { nav.navigate("settings") },
            )
        }
        composable("task/{id}", listOf(navArgument("id") { type = NavType.LongType })) { entry ->
            TaskScreen(entry.arguments!!.getLong("id"), back = { nav.popBackStack("list", false) })
        }
        composable("new") { NewTaskScreen(draft, close = { nav.popBackStack("list", false) }) }
        composable("pair") {
            val before = remember { model.current?.deviceId }
            PairScreen(cancel = { nav.popBackStack("list", false) })
            LaunchedEffect(model.current?.deviceId) { if (model.current?.deviceId != before) nav.popBackStack("list", false) }
        }
        composable("settings") { SettingsScreen(close = { nav.popBackStack("list", false) }) }
    }
}

/** 出错时在顶部显示几秒。 */
@Composable
private fun ProblemBanner() {
    val model = LocalModel.current
    var shown by remember { mutableStateOf<AppError?>(null) }
    val problem = model.problem
    LaunchedEffect(problem) {
        if (problem != null) {
            shown = problem
            delay(5_000)
            model.problem = null
        }
    }
    Box(Modifier.fillMaxSize().statusBarsPadding(), Alignment.TopCenter) {
        AnimatedVisibility(problem != null, enter = slideInVertically { -it } + fadeIn(), exit = slideOutVertically { -it } + fadeOut()) {
            val shape = RoundedCornerShape(16.dp)
            Label(
                shown?.let(model::message).orEmpty(),
                Modifier.padding(16.dp).fillMaxWidth().clip(shape).background(palette.raised).border(1.dp, palette.line, shape)
                    .clickable { model.problem = null }.padding(horizontal = 16.dp, vertical = 12.dp).semantics { liveRegion = LiveRegionMode.Polite },
                size = 14.sp,
            )
        }
    }
}
