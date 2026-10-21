package app.sda.mobile.sda

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.view.ContextThemeWrapper
import android.view.Gravity
import android.widget.PopupMenu
import org.json.JSONArray
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.RenderNode
import android.view.ViewTreeObserver
import android.view.View
import android.view.ViewGroup
import android.net.Uri
import android.os.Build
import androidx.core.view.WindowCompat
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
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
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
import com.kyant.backdrop.catalog.components.LiquidToggle
import com.kyant.backdrop.catalog.components.LiquidSideTabs
import com.kyant.backdrop.catalog.components.LiquidSideTab
import com.kyant.backdrop.catalog.components.LiquidGlassBlurRadius
import com.kyant.backdrop.catalog.components.liquidGlassSurfaceColor
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
        Function("setNavigationBarAppearance") { light: Boolean ->
            val activity = appContext.currentActivity
            if (activity != null) activity.runOnUiThread {
                if (!activity.isDestroyed && !activity.isFinishing) {
                    WindowCompat.getInsetsController(activity.window, activity.window.decorView).isAppearanceLightNavigationBars = light
                }
            }
            Unit
        }
        View(SdaAndroidChromeView::class) {
            Events("onSelect", "onExpand", "onPlayback", "onToggle", "onSpatialSelect", "onThemeSelect", "onThemeDismiss")
            Prop("presentation") { view: SdaAndroidChromeView, value: String -> view.presentation = if (value in listOf("bottom", "rail", "mini", "header", "toggle", "button", "theme-anchor")) value else "bottom" }
            Prop("preserveAccentColor") { view: SdaAndroidChromeView, value: Boolean -> view.preserveAccentColor = value }
            Prop("tabBarWidth") { view: SdaAndroidChromeView, value: Float -> view.tabBarWidth = if (value.isFinite()) value.coerceIn(160f, 480f) else 264f }
            Prop("topInset") { view: SdaAndroidChromeView, value: Float -> view.topInset = value.coerceAtLeast(0f) }
            Prop("checked") { view: SdaAndroidChromeView, value: Boolean -> view.checked = value }
            Prop("blurred") { view: SdaAndroidChromeView, value: Boolean -> view.blurred = value }
            Prop("headerAction") { view: SdaAndroidChromeView, value: Boolean -> view.headerAction = value }
            Prop("selected") { view: SdaAndroidChromeView, value: Int -> view.selected = value.coerceIn(0, 2) }
            Prop("accentColorValue") { view: SdaAndroidChromeView, value: String -> view.accentColor = runCatching { Color(android.graphics.Color.parseColor(value)) }.getOrDefault(Color(0xFF7799CC)) }
            Prop("glassDark") { view: SdaAndroidChromeView, value: Boolean? -> view.glassDark = value }
            Prop("dark") { view: SdaAndroidChromeView, value: Boolean -> view.dark = value }
            Prop("title") { view: SdaAndroidChromeView, value: String -> view.title = value }
            Prop("artist") { view: SdaAndroidChromeView, value: String -> view.artist = value }
            Prop("coverUri") { view: SdaAndroidChromeView, value: String -> view.coverUri = value }
            Prop("hasTrack") { view: SdaAndroidChromeView, value: Boolean -> view.hasTrack = value }
            Prop("playing") { view: SdaAndroidChromeView, value: Boolean -> view.playing = value }
            Prop("busy") { view: SdaAndroidChromeView, value: Boolean -> view.busy = value }
            Prop("spatialButton") { view: SdaAndroidChromeView, value: Boolean -> view.spatialButton = value }
            Prop("spatialEnabled") { view: SdaAndroidChromeView, value: Boolean -> view.spatialEnabled = value }
            Prop("spatialTracking") { view: SdaAndroidChromeView, value: Boolean -> view.spatialTracking = value }
            Prop("headYawDegrees") { view: SdaAndroidChromeView, value: Float -> view.headYawDegrees = if (value.isFinite()) value.coerceIn(-45f,45f) else 0f }
            Prop("themeMenuItems") { view: SdaAndroidChromeView, value: String -> view.themeMenuItems = value }
            Prop("themeMenuSelected") { view: SdaAndroidChromeView, value: String -> view.themeMenuSelected = value }
            Prop("themeMenuRequest") { view: SdaAndroidChromeView, value: Int -> view.themeMenuRequest = value }
            OnViewDidUpdateProps { view: SdaAndroidChromeView -> view.showThemeMenuIfRequested() }
            Prop("compactButton") { view: SdaAndroidChromeView, value: Boolean -> view.compactButton = value }
            Prop("settingsButton") { view: SdaAndroidChromeView, value: Boolean -> view.settingsButton = value }
            Prop("backButton") { view: SdaAndroidChromeView, value: Boolean -> view.backButton = value }
            Prop("backgroundColorValue") { view: SdaAndroidChromeView, value: String -> view.surfaceColor = runCatching { Color(android.graphics.Color.parseColor(value)) }.getOrDefault(Color(0xFFF2F2F7)) }
        }
    }
}

class SdaAndroidChromeView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
    override val shouldUseAndroidLayout = true
    private val onToggle by EventDispatcher()
    private val onSelect by EventDispatcher()
    private val onExpand by EventDispatcher()
    private val onPlayback by EventDispatcher()
    private val onSpatialSelect by EventDispatcher()
    private val onThemeSelect by EventDispatcher()
    private val onThemeDismiss by EventDispatcher()
    var themeMenuItems = "[]"
    var themeMenuSelected = ""
    var themeMenuRequest = 0
    private var presentedThemeRequest = 0
    private var themePopup: PopupMenu? = null

    var topInset by mutableFloatStateOf(0f)
    var checked by mutableStateOf(false)
    var blurred by mutableStateOf(false)
    var headerAction by mutableStateOf(true)
    var presentation by mutableStateOf("bottom")
    var selected by mutableIntStateOf(0)
    var accentColor by mutableStateOf(Color(0xFF7799CC))
    var dark by mutableStateOf(false)
    var glassDark by mutableStateOf<Boolean?>(null)
    var title by mutableStateOf("")
    var artist by mutableStateOf("")
    var coverUri by mutableStateOf("")
    var hasTrack by mutableStateOf(false)
    var playing by mutableStateOf(false)
    var busy by mutableStateOf(false)
    var compactButton by mutableStateOf(false)
    var settingsButton by mutableStateOf(false)
    var spatialButton by mutableStateOf(false)
    var spatialEnabled by mutableStateOf(false)
    var spatialTracking by mutableStateOf(false)
    var headYawDegrees by mutableFloatStateOf(0f)
    var backButton by mutableStateOf(false)
    var preserveAccentColor by mutableStateOf(false)
    var tabBarWidth by mutableFloatStateOf(264f)
    var surfaceColor by mutableStateOf(Color(0xFFF2F2F7))
    private val backgroundNode = if (Build.VERSION.SDK_INT >= 29) RenderNode("SDA glass background") else null
    private var backgroundRevision by mutableIntStateOf(0)
    private val menuBackgroundNode = if (Build.VERSION.SDK_INT >= 29) RenderNode("SDA context menu background") else null
    private var menuBackgroundRevision by mutableIntStateOf(0)
    private var spatialMenuOpen by mutableStateOf(false)
    private var interactionGeneration by mutableIntStateOf(0)
    private var capturing = false
    private var observedTree: ViewTreeObserver? = null
    private val visibleBounds = android.graphics.Rect()
    private val location = IntArray(2)
    private val rootLocation = IntArray(2)
    private val captureListener = ViewTreeObserver.OnPreDrawListener {
        if (!capturing && presentation != "theme-anchor" && isShown && (hasWindowFocus() || spatialMenuOpen) && windowVisibility == VISIBLE && width > 0 && height > 0 && getGlobalVisibleRect(visibleBounds)) {
            recordBackground()
            if (spatialMenuOpen) recordMenuBackground()
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
        themePopup?.dismiss()
        themePopup = null
        observedTree?.takeIf { it.isAlive }?.removeOnPreDrawListener(captureListener)
        observedTree = null
        spatialMenuOpen = false
        menuBackgroundNode?.discardDisplayList()
        backgroundNode?.discardDisplayList()
        super.onDetachedFromWindow()
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        // A focusable menu owns another window. Preserve its composition while
        // that window receives back/outside events.
        if (spatialMenuOpen) return
        backgroundNode?.discardDisplayList()
        interactionGeneration++
        if (hasWindowFocus) postInvalidateOnAnimation()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        if (visibility != VISIBLE) spatialMenuOpen = false
    }

    fun showThemeMenuIfRequested() {
        if (presentation != "theme-anchor" || themeMenuRequest <= 0 || themeMenuRequest == presentedThemeRequest) return
        val request = themeMenuRequest
        presentedThemeRequest = request
        post {
            if (themeMenuRequest != request) return@post
            if (!isAttachedToWindow || windowToken == null || width == 0 || height == 0) {
                onThemeDismiss(emptyMap<String, Any>())
                return@post
            }
            val entries = runCatching { JSONArray(themeMenuItems) }.getOrNull()
            if (entries == null || entries.length() == 0) {
                onThemeDismiss(emptyMap<String, Any>())
                return@post
            }
            themePopup?.dismiss()
            val popupContext = ContextThemeWrapper(context,
                if (dark) android.R.style.Theme_Material else android.R.style.Theme_Material_Light)
            val popup = PopupMenu(popupContext, this, Gravity.END)
            val ids = mutableMapOf<Int, String>()
            val iconSize = (18 * resources.displayMetrics.density + .5f).toInt()
            for (index in 0 until minOf(entries.length(), 16)) {
                val entry = entries.optJSONObject(index) ?: continue
                val id = entry.optString("id")
                val label = entry.optString("label")
                if (id.isBlank() || label.isBlank()) continue
                ids[index + 1] = id
                popup.menu.add(1, index + 1, index, label).apply {
                    isCheckable = true
                    isChecked = id == themeMenuSelected
                    val color = runCatching { android.graphics.Color.parseColor(entry.optString("color")) }.getOrNull()
                    if (color != null) icon = GradientDrawable().apply {
                        shape = GradientDrawable.OVAL; setColor(color); setSize(iconSize, iconSize)
                    }
                }
            }
            if (ids.isEmpty()) {
                onThemeDismiss(emptyMap<String, Any>())
                return@post
            }
            popup.menu.setGroupCheckable(1, true, true)
            if (Build.VERSION.SDK_INT >= 29) popup.setForceShowIcon(true)
            popup.setOnMenuItemClickListener { item ->
                ids[item.itemId]?.let { onThemeSelect(mapOf("id" to it)) }
                true
            }
            popup.setOnDismissListener {
                if (themePopup === popup) themePopup = null
                onThemeDismiss(emptyMap<String, Any>())
            }
            themePopup = popup
            runCatching { popup.show() }.onFailure {
                themePopup = null
                onThemeDismiss(emptyMap<String, Any>())
                android.util.Log.w("SdaAndroidThemeMenu", "Framework popup could not be shown", it)
            }
        }
    }

    private fun recordMenuBackground() {
        val node = menuBackgroundNode ?: return
        val root = rootView
        if (root.width <= 0 || root.height <= 0) return
        root.getLocationInWindow(rootLocation)
        node.setPosition(0, 0, root.width, root.height)
        val canvas = node.beginRecording(root.width, root.height)
        capturing = true
        try {
            drawBackgroundBranch(root, canvas)
        } finally {
            capturing = false
            node.endRecording()
        }
        menuBackgroundRevision++
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
        if (presentation == "theme-anchor") return
        val ink = if (dark) Color(0xFFF5F5F7) else Color(0xFF1C1C1E)
        val muted = if (dark) Color(0xFFA0A0A8) else Color(0xFF74747C)
        val accent = accentColor
        val navigationSurface = liquidGlassSurfaceColor(glassDark ?: dark)
        val showMini = hasTrack && selected != 0 && presentation != "rail"
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
        if (presentation == "button") {
            LiquidButton(onClick = { if (!busy) onExpand(emptyMap<String, Any>()) }, backdrop = backgroundAt(0f),
                enabled = !busy, label = title, height = 44.dp, contentPadding = 12.dp,
                modifier = Modifier.fillMaxWidth().drawWithContent { backgroundRevision; drawContent() }) {
                Text(title, color = accent, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            }
            return
        }
        if (presentation == "toggle") {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                LiquidToggle(selected = { checked }, onSelect = { if (!busy) onToggle(mapOf("value" to it)) },
                    backdrop = backgroundAt(0f), dark = dark, accentColor = accent, enabled = !busy, label = title,
                    modifier = Modifier.drawWithContent { backgroundRevision; drawContent() })
            }
            return
        }
        if (presentation == "header") {
            val backdrop = backgroundAt(0f)
            val density = androidx.compose.ui.platform.LocalDensity.current
            val buttonBackdrop = backgroundAt(with(density) { (topInset.dp + 6.dp).toPx() }, if (backButton) with(density) { 6.dp.toPx() } else width.toFloat() - with(density) { 50.dp.toPx() })
            val surface = if (blurred) Modifier.drawBackdrop(backdrop = backdrop, shape = { RectangleShape },
                highlight = { null }, shadow = { null }, effects = { blur(20.dp.toPx()) },
                onDrawSurface = { drawRect(surfaceColor.copy(alpha = .35f)) }) else Modifier
            Box(Modifier.fillMaxSize().then(surface).drawWithContent { backgroundRevision; drawContent() }.padding(top = topInset.dp), contentAlignment = Alignment.Center) {
                Text(title, color = ink, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1,
                    modifier = Modifier.padding(horizontal = 56.dp).semantics { heading() }, overflow = TextOverflow.Ellipsis)
                if (headerAction) Box(Modifier.align(if (backButton) Alignment.CenterStart else Alignment.CenterEnd).padding(horizontal = 6.dp)) {
                    LiquidButton(onClick = { onExpand(emptyMap<String, Any>()) }, backdrop = buttonBackdrop,
                        surfaceColor = navigationSurface, blurRadius = LiquidGlassBlurRadius,
                        label = if (backButton) "返回" else "更多设置", height = 44.dp, contentPadding = 0.dp, modifier = Modifier.width(44.dp)) {
                        Icon(if (backButton) Icons.AutoMirrored.Filled.ArrowBack else Icons.Default.Settings, null, Modifier.size(22.dp), tint = ink)
                    }
                }
            }
            return
        }
        if (settingsButton) {
            LaunchedEffect(busy, spatialButton) {
                if (busy || !spatialButton) spatialMenuOpen = false
            }
            val headAngle by animateFloatAsState(if (spatialButton && spatialEnabled && spatialTracking) headYawDegrees else 0f, tween(100), label = "spatialHeadYaw")
            // Keep a 48dp handset hit lane while reducing only the glass
            // circle. Compose expands this clickable into the available lane.
            val actionSize = if (compactButton) 36.dp else 44.dp
            val actionIconSize = if (compactButton) 20.dp else 22.dp
            val actionInsetPx = with(androidx.compose.ui.platform.LocalDensity.current) { (((if (compactButton) 48.dp else 44.dp) - actionSize) / 2).toPx() }
            val backdrop = backgroundAt(actionInsetPx, actionInsetPx)
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                LiquidButton(onClick = {
                    if (spatialButton) {
                        recordMenuBackground()
                        spatialMenuOpen = true
                    } else onExpand(emptyMap<String, Any>())
                }, backdrop = backdrop,
                    surfaceColor = navigationSurface, blurRadius = LiquidGlassBlurRadius,
                    enabled = !busy, label = if (backButton) "返回" else if (spatialButton) "空间音频" else "更多设置", height = actionSize, contentPadding = 0.dp,
                    modifier = Modifier.width(actionSize).graphicsLayer { alpha = if (spatialMenuOpen && spatialButton) 0f else 1f }.drawWithContent { backgroundRevision; drawContent() }) {
                    Icon(if (backButton) Icons.AutoMirrored.Filled.ArrowBack else if (spatialButton) Icons.Default.AccountCircle else Icons.Default.Settings, null, Modifier.size(actionIconSize).graphicsLayer { rotationY = headAngle }, tint = if (spatialButton && spatialEnabled) accent else ink)
                }
                if (spatialMenuOpen && spatialButton && !busy) {
                    var menuOffset by remember { mutableStateOf(IntOffset.Zero) }
                    val menuBackdrop = rememberCanvasBackdrop {
                        menuBackgroundRevision
                        val node = menuBackgroundNode
                        if (node != null && node.hasDisplayList() && drawContext.canvas.nativeCanvas.isHardwareAccelerated) {
                            translate(left = (rootLocation[0] - menuOffset.x).toFloat(), top = (rootLocation[1] - menuOffset.y).toFloat()) {
                                drawContext.canvas.nativeCanvas.drawRenderNode(node)
                            }
                        }
                    }
                    SdaSpatialContextMenu(
                        enabled = spatialEnabled, dark = glassDark ?: dark, accent = accent,
                        backdrop = menuBackdrop, hasBackdrop = Build.VERSION.SDK_INT >= 31,
                        onPosition = { menuOffset = it },
                        onDismiss = { spatialMenuOpen = false },
                        onSelect = { enabled ->
                            if (!busy) onSpatialSelect(mapOf("enabled" to enabled))
                        }
                    )
                }
            }
            return
        }
        if (presentation == "rail") {
            val selection = remember { { selected } }
            LiquidSideTabs(selectedTabIndex = selection, onTabSelected = { index ->
                if (index != selected) onSelect(mapOf("index" to index))
            }, backdrop = backgroundAt(0f), tabsCount = 3, dark = dark, accentColor = accent, preserveAccentColor = preserveAccentColor,
                modifier = Modifier.fillMaxSize().drawWithContent { backgroundRevision; drawContent() }) {
                val labels = listOf("播放", "资料库", "空间")
                val icons = listOf(Icons.Default.PlayCircle, Icons.Default.LibraryMusic, Icons.Default.ViewInAr)
                labels.forEachIndexed { index, label ->
                    LiquidSideTab(onClick = { if (selected != index) onSelect(mapOf("index" to index)) },
                        modifier = Modifier.semantics { this.selected = this@SdaAndroidChromeView.selected == index }) {
                        Icon(icons[index], label, Modifier.size(23.dp), tint = if (selected == index) accent else ink)
                    }
                }
            }
            return
        }
        val miniOnly = presentation == "mini"
        val miniBackdrop = backgroundAt(with(androidx.compose.ui.platform.LocalDensity.current) { (if (miniOnly) 0.dp else 8.dp).toPx() })
        val tabsLeft = ((width - with(androidx.compose.ui.platform.LocalDensity.current) { tabBarWidth.dp.toPx() }) / 2f).coerceAtLeast(0f)
        val tabsBackdrop = backgroundAt(with(androidx.compose.ui.platform.LocalDensity.current) { (if (showMini) 62.dp else 8.dp).toPx() }, left = tabsLeft)
        Column(Modifier.fillMaxSize().padding(top = if (miniOnly) 0.dp else 8.dp, bottom = if (miniOnly) 0.dp else 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
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
            if (miniOnly) return@Column
            // UIKit tab labels use a fixed logical size. Convert this dp
            // baseline through the device's font scaling instead of multiplying
            // an sp size again; body text keeps the user's system scaling.
            val tabLabelSize = with(androidx.compose.ui.platform.LocalDensity.current) { 10.dp.toSp() }
            val tabLabelLineHeight = with(androidx.compose.ui.platform.LocalDensity.current) { 12.dp.toSp() }
            val selection = remember { { selected } }
            LiquidBottomTabs(selectedTabIndex = selection, onTabSelected = { index ->
                if (index != selected) onSelect(mapOf("index" to index))
            }, backdrop = tabsBackdrop, tabsCount = 3, dark = dark, accentColor = accent, height = 56.dp, preserveAccentColor = preserveAccentColor,
                modifier = Modifier.align(Alignment.CenterHorizontally).widthIn(max = tabBarWidth.dp).fillMaxWidth().height(56.dp).drawWithContent { backgroundRevision; drawContent() }) {
                    val labels = listOf("播放", "资料库", "空间")
                    val icons = listOf(Icons.Default.PlayCircle, Icons.Default.LibraryMusic, Icons.Default.ViewInAr)
                    val currentSelected = selected
                    labels.forEachIndexed { index, label ->
                        LiquidBottomTab(onClick = { if (currentSelected != index) onSelect(mapOf("index" to index)) }, modifier = Modifier.semantics { this.selected = currentSelected == index }) {
                            Icon(icons[index], null, Modifier.size(23.dp), tint = if (selected == index) accent else ink)
                            Text(label, fontSize = tabLabelSize, lineHeight = tabLabelLineHeight, fontWeight = FontWeight.Medium,
                                style = androidx.compose.ui.text.TextStyle(platformStyle = androidx.compose.ui.text.PlatformTextStyle(includeFontPadding = false)),
                                color = if (selected == index) accent else ink, maxLines = 1)
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
