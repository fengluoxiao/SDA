package app.sda.mobile.sda

import android.content.Context
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.Icon
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.views.ExpoView

class SdaAndroidSymbolModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("SdaAndroidSymbol")
        View(SdaAndroidSymbolView::class) {
            Prop("name") { view: SdaAndroidSymbolView, value: String -> view.symbol = value }
            Prop("tint") { view: SdaAndroidSymbolView, value: String -> view.tint = runCatching { Color(android.graphics.Color.parseColor(value)) }.getOrDefault(Color.White) }
        }
    }
}

class SdaAndroidSymbolView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
    override val shouldUseAndroidLayout = true
    var symbol by mutableStateOf("music.note")
    var tint by mutableStateOf(Color.White)
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        if (!isAttachedToWindow) {
            setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), MeasureSpec.getSize(heightMeasureSpec))
            return
        }
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        requestLayout()
    }
    init {
        addView(ComposeView(context).apply {
            layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)
            setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnDetachedFromWindowOrReleasedFromPool)
            setContent {
                val icon = when (symbol) {
                    "play.fill" -> Icons.Default.PlayArrow
                    "pause.fill" -> Icons.Default.Pause
                    "backward.fill" -> Icons.Default.FastRewind
                    "forward.fill" -> Icons.Default.FastForward
                    "speaker.fill" -> Icons.Default.VolumeDown
                    "speaker.wave.2.fill" -> Icons.Default.VolumeUp
                    "gearshape" -> Icons.Default.Settings
                    "chevron.left" -> Icons.Default.ChevronLeft
                    "arrow.left" -> Icons.AutoMirrored.Filled.ArrowBack
                    "chevron.right" -> Icons.Default.ChevronRight
                    "chevron.up.chevron.down" -> Icons.Default.UnfoldMore
                    "info.circle" -> Icons.Default.Info
                    "checkmark" -> Icons.Default.Check
                    "repeat" -> Icons.Default.Repeat
                    "repeat.1" -> Icons.Default.RepeatOne
                    "list.bullet" -> Icons.Default.List
                    "arrow.counterclockwise" -> Icons.Default.Replay
                    "stop.fill" -> Icons.Default.Stop
                    "folder.open", "folder.badge.plus" -> Icons.Default.FolderOpen
                    else -> Icons.Default.MusicNote
                }
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Icon(icon, null, Modifier.fillMaxSize(), tint = tint) }
            }
        })
    }
}
