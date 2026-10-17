package app.sda.mobile.sda

import android.content.Context
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.RenderNode
import android.view.ViewTreeObserver
import android.view.View
import android.view.ViewGroup
import android.net.Uri
import android.os.Build
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.Icon
import androidx.compose.material.Text
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.PlayCircle
import androidx.compose.material.icons.filled.ViewInAr
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.drawscope.translate
import com.kyant.backdrop.Backdrop
import com.kyant.backdrop.backdrops.rememberCanvasBackdrop
import com.kyant.backdrop.drawBackdrop
import com.kyant.backdrop.effects.blur
import com.kyant.backdrop.effects.lens
import com.kyant.backdrop.catalog.components.LiquidBottomTabs
import com.kyant.backdrop.catalog.components.LiquidBottomTab
import com.kyant.backdrop.catalog.components.LiquidButton
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class SdaAndroidChromeModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("SdaAndroidChrome")
        Function("effectLevel") { if (Build.VERSION.SDK_INT >= 33) "refraction" else if (Build.VERSION.SDK_INT >= 31) "blur" else "solid" }
        View(SdaAndroidChromeView::class) {
            Events("onSelect", "onExpand", "onPlayback")
            Prop("selected") { view: SdaAndroidChromeView, value: Int -> view.selected = value.coerceIn(0, 2) }
            Prop("accentColorValue") { view: SdaAndroidChromeView, value: String -> view.accentColor = runCatching { Color(android.graphics.Color.parseColor(value)) }.getOrDefault(Color(0xFF7799CC)) }
            Prop("dark") { view: SdaAndroidChromeView, value: Boolean -> view.dark = value }
            Prop("title") { view: SdaAndroidChromeView, value: String -> view.title = value }
            Prop("artist") { view: SdaAndroidChromeView, value: String -> view.artist = value }
            Prop("coverUri") { view: SdaAndroidChromeView, value: String -> view.coverUri = value }
            Prop("hasTrack") { view: SdaAndroidChromeView, value: Boolean -> view.hasTrack = value }
            Prop("playing") { view: SdaAndroidChromeView, value: Boolean -> view.playing = value }
            Prop("busy") { view: SdaAndroidChromeView, value: Boolean -> view.busy = value }
            Prop("settingsButton") { view: SdaAndroidChromeView, value: Boolean -> view.settingsButton = value }
            Prop("backButton") { view: SdaAndroidChromeView, value: Boolean -> view.backButton = value }
            Prop("backgroundColorValue") { view: SdaAndroidChromeView, value: String -> view.surfaceColor = runCatching { Color(android.graphics.Color.parseColor(value)) }.getOrDefault(Color(0xFFF2F2F7)) }
        }
    }
}

class SdaAndroidChromeView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
    override val shouldUseAndroidLayout = true
    private val onSelect by EventDispatcher()
    private val onExpand by EventDispatcher()
    private val onPlayback by EventDispatcher()
    var selected by mutableIntStateOf(0)
    var accentColor by mutableStateOf(Color(0xFF7799CC))
    var dark by mutableStateOf(false)
    var title by mutableStateOf("")
    var artist by mutableStateOf("")
    var coverUri by mutableStateOf("")
    var hasTrack by mutableStateOf(false)
    var playing by mutableStateOf(false)
    var busy by mutableStateOf(false)
    var settingsButton by mutableStateOf(false)
    var backButton by mutableStateOf(false)
    var surfaceColor by mutableStateOf(Color(0xFFF2F2F7))
    private val backgroundNode = if (Build.VERSION.SDK_INT >= 29) RenderNode("SDA glass background") else null
    private var backgroundRevision by mutableIntStateOf(0)
    private var interactionGeneration by mutableIntStateOf(0)
    private var capturing = false
    private var observedTree: ViewTreeObserver? = null
    private val location = IntArray(2)
    private val rootLocation = IntArray(2)
    private val captureListener = ViewTreeObserver.OnPreDrawListener {
        if (!capturing && isShown && hasWindowFocus() && windowVisibility == VISIBLE && width > 0 && height > 0) {
            recordBackground()
        }
        true
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        requestLayout()
        observedTree = rootView.viewTreeObserver.also { it.addOnPreDrawListener(captureListener) }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        if (!isAttachedToWindow) {
            setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), MeasureSpec.getSize(heightMeasureSpec))
            return
        }
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
    }

    override fun onDetachedFromWindow() {
        observedTree?.takeIf { it.isAlive }?.removeOnPreDrawListener(captureListener)
        observedTree = null
        backgroundNode?.discardDisplayList()
        super.onDetachedFromWindow()
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        backgroundNode?.discardDisplayList()
        interactionGeneration++
        if (hasWindowFocus) postInvalidateOnAnimation()
    }

    override fun draw(canvas: Canvas) {
        if (!capturing) super.draw(canvas)
    }

    override fun dispatchDraw(canvas: Canvas) {
        if (!capturing) super.dispatchDraw(canvas)
    }

    private fun recordBackground() {
        val backgroundNode = backgroundNode ?: return
        // Hardware display-list recording: no screenshot, pixel readback or Bitmap.
        // Separate SurfaceView/GL surfaces are not part of the View display list.
        val root = rootView
        getLocationInWindow(location)
        root.getLocationInWindow(rootLocation)
        backgroundNode.setPosition(0, 0, width, height)
        val canvas = backgroundNode.beginRecording(width, height)
        canvas.translate(-(location[0] - rootLocation[0]).toFloat(), -(location[1] - rootLocation[1]).toFloat())
        capturing = true
        try {
            drawBackgroundBranch(root, canvas)
        } finally {
            capturing = false
            backgroundNode.endRecording()
        }
        backgroundRevision++
    }

    private fun drawBackgroundBranch(view: View, canvas: Canvas) {
        if (view is SdaAndroidChromeView || view.visibility != VISIBLE) return
        if (view !is ViewGroup) {
            view.draw(canvas)
            return
        }
        // Walk current scroll coordinates instead of reusing container display
        // lists, which can retain an older scroll position or reference us.
        view.background?.draw(canvas)
        val clipSave = canvas.save()
        if (view.clipChildren) canvas.clipRect(0, 0, view.width, view.height)
        for (index in 0 until view.childCount) {
            val child = view.getChildAt(index)
            if (child === this) continue
            val save = canvas.save()
            canvas.translate((child.left - view.scrollX).toFloat(), (child.top - view.scrollY).toFloat())
            canvas.concat(child.matrix)
            drawBackgroundBranch(child, canvas)
            canvas.restoreToCount(save)
        }
        canvas.restoreToCount(clipSave)
    }

    init {
        clipChildren = false
        clipToPadding = false
        addView(ComposeView(context).apply {
            layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)
            setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnDetachedFromWindowOrReleasedFromPool)
            setContent { key(interactionGeneration) { Chrome() } }
        })
    }

    @Composable
    private fun Chrome() {
        val ink = if (dark) Color(0xFFF5F5F7) else Color(0xFF171C19)
        val muted = if (dark) Color(0xFFA0AAA4) else Color(0xFF727C75)
        val accent = accentColor
        val showMini = hasTrack && selected != 0
        val cover by produceState<ImageBitmap?>(null, coverUri) {
            value = null
            if (coverUri.isNotBlank()) {
                value = withContext(Dispatchers.IO) {
                    runCatching {
                        val uri = Uri.parse(coverUri)
                        // Local artwork only; do not perform network I/O on a playback control.
                        if (uri.scheme !in listOf("file", "content", "android.resource")) return@runCatching null
                        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                        context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
                        val options = BitmapFactory.Options().apply {
                            var sample = 1
                            while (bounds.outWidth / sample > 256 || bounds.outHeight / sample > 256) sample *= 2
                            inSampleSize = sample
                        }
                        context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options)?.asImageBitmap() }
                    }.getOrNull()
                }
            }
        }
        @Composable
        fun backgroundAt(top: Float, left: Float = 0f): Backdrop = rememberCanvasBackdrop {
            backgroundRevision
            val node = backgroundNode
            if (node != null && node.hasDisplayList() && drawContext.canvas.nativeCanvas.isHardwareAccelerated) {
                translate(left = -left, top = -top) {
                    drawContext.canvas.nativeCanvas.drawRenderNode(node)
                }
            }
        }
        val revision = backgroundRevision
        if (settingsButton) {
            val backdrop = backgroundAt(0f)
            LiquidButton(onClick = { onExpand(emptyMap<String, Any>()) }, backdrop = backdrop,
                enabled = !busy, label = if (backButton) "返回" else "更多设置", height = 44.dp, contentPadding = 0.dp,
                modifier = Modifier.fillMaxWidth().drawWithContent { backgroundRevision; drawContent() }) {
                Icon(if (backButton) Icons.AutoMirrored.Filled.ArrowBack else Icons.Default.Settings, null, Modifier.size(22.dp), tint = ink)
            }
            return
        }
        val miniBackdrop = backgroundAt(with(androidx.compose.ui.platform.LocalDensity.current) { 8.dp.toPx() })
        val tabsBackdrop = backgroundAt(with(androidx.compose.ui.platform.LocalDensity.current) { (if (showMini) 62.dp else 8.dp).toPx() })
        Column(Modifier.fillMaxSize().padding(top = 8.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            if (showMini) {
                LiquidButton(onClick = { onExpand(emptyMap<String, Any>()) }, backdrop = miniBackdrop, label = "展开正在播放", height = 48.dp, contentPadding = 10.dp,
                    modifier = Modifier.fillMaxWidth().drawWithContent { backgroundRevision; drawContent() }) {
                        Box(Modifier.size(32.dp).clip(RoundedCornerShape(8.dp)).background(ink.copy(alpha = .06f)), contentAlignment = Alignment.Center) {
                            if (cover != null) Image(cover!!, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
                            else Icon(Icons.Default.MusicNote, null, Modifier.size(25.dp), tint = muted)
                        }
                        Column(Modifier.weight(1f)) {
                            Text(title, color = ink, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(artist, color = muted, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    Box(Modifier.padding(start = 4.dp, end = 6.dp).size(48.dp)
                        .clickable(interactionSource = null, indication = null, enabled = !busy, role = Role.Button,
                            onClickLabel = if (playing) "暂停" else "播放") { onPlayback(emptyMap<String, Any>()) },
                        contentAlignment = Alignment.Center) {
                        Icon(if (playing) Icons.Default.Pause else Icons.Default.PlayArrow, if (playing) "暂停" else "播放", Modifier.size(24.dp), tint = accent.copy(alpha = if (busy) .35f else 1f))
                    }
                }
            }
            val selection = remember { { selected } }
            LiquidBottomTabs(selectedTabIndex = selection, onTabSelected = { index ->
                if (index != selected) onSelect(mapOf("index" to index))
            }, backdrop = tabsBackdrop, tabsCount = 3, dark = dark, accentColor = accent, height = 56.dp,
                modifier = Modifier.fillMaxWidth().height(56.dp).drawWithContent { backgroundRevision; drawContent() }) {
                    val labels = listOf("播放", "资料库", "空间")
                    val icons = listOf(Icons.Default.PlayCircle, Icons.Default.LibraryMusic, Icons.Default.ViewInAr)
                    val currentSelected = selected
                    labels.forEachIndexed { index, label ->
                        LiquidBottomTab(onClick = { if (currentSelected != index) onSelect(mapOf("index" to index)) }, modifier = Modifier.semantics { this.selected = currentSelected == index }) {
                            Icon(icons[index], null, Modifier.size(23.dp), tint = if (selected == index) accent else ink)
                            Text(label, fontSize = 10.sp, fontWeight = if (selected == index) FontWeight.SemiBold else FontWeight.Medium, color = if (selected == index) accent else ink, maxLines = 1)
                        }
                    }
            }
        }
    }
}

private fun Modifier.glass(backdrop: Backdrop, revision: Int): Modifier = drawBackdrop(
    backdrop = backdrop,
    shape = { RoundedCornerShape(50) },
    effects = { revision.hashCode(); blur(8.dp.toPx()); lens(18.dp.toPx(), 12.dp.toPx()) }
)
