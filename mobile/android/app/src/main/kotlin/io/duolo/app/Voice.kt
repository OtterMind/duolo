package io.duolo.app

import io.duolo.app.core.AppError
import io.duolo.app.core.VoiceHints
import io.duolo.app.core.VoiceStream
import android.annotation.SuppressLint
import android.Manifest
import android.app.Application
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.SystemClock
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LocalTextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import kotlin.math.sqrt

/**
 * 一次语音输入：从按下到说完。录麦克风，16 kHz PCM 发给配对的 Duolo 服务器识别（和电脑端同一个接口），
 * 带上任务的热词和对话。状态只在主线程上改。
 */
class VoiceSession(private val app: Application, private val http: OkHttpClient) {
    enum class Target { Send, Edit, Cancel }

    var visible by mutableStateOf(false)
        private set

    /** 轻点开始的：一直听，用面板上的按钮结束。 */
    var handsFree by mutableStateOf(false)
        private set
    var target by mutableStateOf(Target.Send)
    var text by mutableStateOf("")
        private set
    var level by mutableFloatStateOf(0f)
        private set
    var startedAt by mutableLongStateOf(0L)
        private set

    private val scope = CoroutineScope(Dispatchers.Main.immediate)
    private var stream: VoiceStream? = null
    private var session: Job? = null
    private var recording: Job? = null
    private var done: (String, Boolean) -> Unit = { _, _ -> }
    private var problem: (AppError) -> Unit = {}

    /** 已经松手了，在等最终结果。 */
    private var finishing = false
    private var timeout: Job? = null

    val allowed: Boolean
        get() = ContextCompat.checkSelfPermission(app, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    /** `route` 是配对的服务器和登录凭据，没配对时为空。 */
    fun begin(route: Pair<String, String>?, hints: VoiceHints, problem: (AppError) -> Unit, done: (text: String, edit: Boolean) -> Unit) {
        if (visible) return
        if (route == null) return problem(AppError.unauthorized)
        this.done = done
        this.problem = problem
        target = Target.Send
        handsFree = false
        finishing = false
        text = ""
        level = 0f
        startedAt = SystemClock.elapsedRealtime()
        visible = true
        session = scope.launch {
            val opened = try {
                VoiceStream.open(route.first, route.second, hints, http)
            } catch (e: AppError) {
                end()
                return@launch problem(e)
            }
            // 连服务器的时候已经取消或松手了。
            if (!visible || finishing) {
                opened.cancel()
                if (finishing) complete()
                return@launch
            }
            stream = opened
            recording = record(opened)
            for (event in opened.events) {
                when (event) {
                    is VoiceStream.Event.Partial -> text = event.text
                    is VoiceStream.Event.Final -> {
                        text = event.text
                        if (finishing) complete()
                    }
                    is VoiceStream.Event.Failed -> if (finishing) complete() else {
                        end()
                        problem(event.error)
                    }
                }
            }
        }
    }

    /** 在后台线程上录音，每 100 毫秒发一块。 */
    @SuppressLint("MissingPermission") // 开始前 HoldToTalk 已经确认过权限。
    private fun record(stream: VoiceStream) = scope.launch(Dispatchers.IO) {
        val chunk = RATE / 10 * 2
        val size = maxOf(AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT), chunk * 2)
        val recorder = try {
            AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, size)
        } catch (_: Exception) {
            null
        }
        if (recorder == null || recorder.state != AudioRecord.STATE_INITIALIZED) {
            recorder?.release()
            launch(Dispatchers.Main) {
                end()
                problem(AppError("voicePermission"))
            }
            return@launch
        }
        val buffer = ByteArray(chunk)
        try {
            recorder.startRecording()
            while (isActive) {
                val n = recorder.read(buffer, 0, buffer.size)
                if (n <= 0) continue
                stream.send(buffer, n)
                val loud = rms(buffer, n)
                launch(Dispatchers.Main) { level = loud }
            }
        } finally {
            recorder.stop()
            recorder.release()
        }
    }

    fun keepListening() {
        handsFree = true
        target = Target.Send
    }

    fun finish(target: Target) {
        if (!visible || finishing) return
        if (target == Target.Cancel) return end()
        this.target = target
        finishing = true
        recording?.cancel()
        stream?.stop()
        timeout = scope.launch {
            delay(FINAL_TIMEOUT_MS)
            complete()
        }
    }

    private fun complete() {
        if (!visible) return
        val said = text.trim()
        val edit = target == Target.Edit
        end()
        if (said.isEmpty()) problem(AppError("nothingRecognized")) else done(said, edit)
    }

    private fun end() {
        timeout?.cancel()
        recording?.cancel()
        stream?.cancel()
        session?.cancel()
        stream = null
        recording = null
        session = null
        finishing = false
        visible = false
        level = 0f
    }

    private companion object {
        const val RATE = 16_000

        /** 说完后等最终结果最长多久。 */
        const val FINAL_TIMEOUT_MS = 20_000L

        /** 音量（0~1）：均方根放大后截断。 */
        fun rms(pcm: ByteArray, size: Int): Float {
            var sum = 0.0
            val samples = size / 2
            for (i in 0 until samples) {
                val s = ((pcm[2 * i + 1].toInt() shl 8) or (pcm[2 * i].toInt() and 0xff)).toShort() / 32768.0
                sum += s * s
            }
            return if (samples == 0) 0f else (sqrt(sum / samples) * 12).toFloat().coerceIn(0f, 1f)
        }
    }
}

/** 按住说话：松开发送，上滑先改，左滑取消。轻点一下则一直听，用面板上的按钮结束（读屏也用这个）。 */
@Composable
fun HoldToTalk(modifier: Modifier = Modifier, floating: Boolean = false, done: (text: String, edit: Boolean) -> Unit) {
    val model = LocalModel.current
    val voice = model.voice
    val haptics = LocalHapticFeedback.current
    val onDone by rememberUpdatedState(done)
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (!granted) model.problem = AppError("voicePermission")
    }

    /** 开始听；没有权限时先要权限，这一次不录。 */
    fun begin(): Boolean {
        if (!voice.allowed) {
            permission.launch(Manifest.permission.RECORD_AUDIO)
            return false
        }
        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
        voice.begin(model.voiceRoute(), model.voiceHints(), { model.problem = it }) { text, edit -> onDone(text, edit) }
        return voice.visible
    }

    Box(
        modifier.size(if (floating) 56.dp else 48.dp).clip(CircleShape).background(if (floating) palette.green else Color.Transparent)
            .semantics {
                contentDescription = model.t("mobile.voice.hold")
                role = Role.Button
                onClick {
                    if (begin()) voice.keepListening()
                    true
                }
            }
            .pointerInput(Unit) {
                val slide = 70.dp.toPx()
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    if (!begin()) return@awaitEachGesture
                    val pressedAt = SystemClock.elapsedRealtime()
                    while (true) {
                        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        change.consume()
                        if (!change.pressed) break
                        val moved = change.position - down.position
                        val next = when {
                            moved.y < -slide -> VoiceSession.Target.Edit
                            moved.x < -slide -> VoiceSession.Target.Cancel
                            else -> VoiceSession.Target.Send
                        }
                        if (next != voice.target) {
                            voice.target = next
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        }
                    }
                    if (SystemClock.elapsedRealtime() - pressedAt < 350 && voice.target == VoiceSession.Target.Send) {
                        voice.keepListening()
                    } else {
                        voice.finish(voice.target)
                    }
                }
            },
        Alignment.Center,
    ) { Icon(R.drawable.ic_mic, size = 22.dp, tint = if (floating) palette.onGreen else palette.text) }
}

/** 说话时的面板，从底部升起；背后的内容不压暗，边看边答。 */
@Composable
fun VoiceOverlay() {
    val voice = LocalModel.current.voice
    BackHandler(voice.visible) { voice.finish(VoiceSession.Target.Cancel) }
    Box(Modifier.fillMaxSize(), Alignment.BottomCenter) {
        AnimatedVisibility(voice.visible, enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut()) { VoicePanel() }
    }
}

@Composable
private fun VoicePanel() {
    val model = LocalModel.current
    val t = model.t
    val voice = model.voice
    val cancelling = voice.target == VoiceSession.Target.Cancel
    val editing = voice.target == VoiceSession.Target.Edit
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp)).background(palette.raised).navigationBarsPadding().padding(24.dp),
        Arrangement.spacedBy(24.dp),
    ) {
        val empty = voice.text.isEmpty()
        Label(
            if (empty) t("mobile.voice.hint") else voice.text, Modifier.fillMaxWidth().defaultMinSize(minHeight = 60.dp), size = 20.sp,
            color = if (empty || cancelling) palette.muted else palette.text,
            style = LocalTextStyle.current.copy(textDecoration = if (cancelling && !empty) TextDecoration.LineThrough else null),
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (voice.handsFree) {
                IconButton(R.drawable.ic_x, t("common.cancel")) { voice.finish(VoiceSession.Target.Cancel) }
                Spacer(Modifier.weight(1f))
                Clock()
                IconButton(R.drawable.ic_pencil, t("mobile.voice.releaseEdit")) { voice.finish(VoiceSession.Target.Edit) }
                Box(
                    Modifier.size(56.dp).clip(CircleShape).background(palette.green).clickable(role = Role.Button) { voice.finish(VoiceSession.Target.Send) }
                        .semantics { contentDescription = t("mobile.send") },
                    Alignment.Center,
                ) { Icon(R.drawable.ic_arrow_up, size = 24.dp, tint = palette.onGreen) }
            } else {
                Column(Modifier.weight(1f), Arrangement.spacedBy(8.dp)) {
                    Hint(R.drawable.ic_arrow_up, t(if (editing) "mobile.voice.releaseEdit" else "mobile.voice.slideEdit"), editing, palette.greenText)
                    Hint(R.drawable.ic_arrow_left, t(if (cancelling) "mobile.voice.releaseCancel" else "mobile.voice.slideCancel"), cancelling, palette.bad)
                }
                Clock()
                Box(
                    Modifier.size(72.dp).clip(CircleShape).background(palette.button.copy(alpha = 0.5f + voice.level / 2))
                        .semantics { contentDescription = t("mobile.voice.releaseSend") },
                    Alignment.Center,
                ) {
                    Box(Modifier.size(56.dp).clip(CircleShape).background(if (cancelling) palette.bad else palette.green), Alignment.Center) {
                        val icon = if (cancelling) R.drawable.ic_x else if (editing) R.drawable.ic_pencil else R.drawable.ic_mic
                        Icon(icon, size = 24.dp, tint = palette.onGreen)
                    }
                }
            }
        }
    }
}

@Composable
private fun Hint(icon: Int, text: String, on: Boolean, color: Color) =
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, size = 16.dp, tint = if (on) color else palette.muted)
        Label(text, size = 14.sp, weight = if (on) FontWeight.SemiBold else FontWeight.Normal, color = if (on) color else palette.muted)
    }

@Composable
private fun Clock() {
    val voice = LocalModel.current.voice
    var seconds by remember { mutableLongStateOf(0L) }
    LaunchedEffect(voice.startedAt) {
        while (true) {
            seconds = (SystemClock.elapsedRealtime() - voice.startedAt) / 1000
            delay(250)
        }
    }
    Label("%d:%02d".format(seconds / 60, seconds % 60), size = 14.sp, color = palette.muted)
}
