package app.sda.mobile.sda

// Adapted from LiquidMenu.kt / LiquidMorphDemo by Aarav90-cpu:
// https://github.com/Kyant0/AndroidLiquidGlass/issues/70
// See BACKDROP-NOTICE.md for the source and SDA integration changes.

import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.Icon
import androidx.compose.material.Text
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Check
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.*
import androidx.compose.ui.util.lerp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import com.kyant.backdrop.Backdrop
import com.kyant.backdrop.drawBackdrop
import com.kyant.backdrop.effects.blur
import com.kyant.backdrop.effects.lens
import com.kyant.backdrop.effects.vibrancy
import com.kyant.shapes.RoundedCornerStyle
import com.kyant.shapes.RoundedRectangle

@Composable
internal fun SdaSpatialContextMenu(
    enabled: Boolean,
    dark: Boolean,
    accent: Color,
    backdrop: Backdrop,
    hasBackdrop: Boolean,
    onPosition: (IntOffset) -> Unit,
    onDismiss: () -> Unit,
    onSelect: (Boolean) -> Unit
) {
    val options = remember { listOf(false to "关闭", true to "固定") }
    var isOpen by remember { mutableStateOf(false) }
    val dismiss by rememberUpdatedState(onDismiss)
    val progress by animateFloatAsState(
        targetValue = if (isOpen) 1f else 0f,
        animationSpec = spring(
            dampingRatio = Spring.DampingRatioLowBouncy,
            stiffness = Spring.StiffnessLow
        ),
        label = "MorphProgress",
        finishedListener = { if (!isOpen) dismiss() }
    )
    LaunchedEffect(Unit) { isOpen = true }
    var anchor by remember { mutableStateOf(IntRect.Zero) }
    val positionProvider = remember {
        object : PopupPositionProvider {
            override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
                anchor = anchorBounds
                return IntOffset.Zero
            }
        }
    }
    // A full-window overlay leaves room for the original spring overshoot.
    // Its transparent area dismisses the menu and does not darken the player.
    Popup(
        popupPositionProvider = positionProvider,
        onDismissRequest = { isOpen = false },
        properties = PopupProperties(focusable = true, dismissOnBackPress = true,
            dismissOnClickOutside = true, clippingEnabled = false)
    ) {
        BoxWithConstraints(
            Modifier.fillMaxSize().semantics { paneTitle = "空间音频" }
                .pointerInput(Unit) { detectTapGestures { isOpen = false } }
        ) {
            val density = LocalDensity.current
            val expandedWidth = minOf(224.dp, (maxWidth - 24.dp).coerceAtLeast(44.dp))
            val expandedHeight = minOf((44 * options.size + 8 * 2).dp, (maxHeight - 24.dp).coerceAtLeast(44.dp))
            // The original demo starts at 64dp; SDA's existing trigger is 44dp.
            // Size the menu from two 44dp rows plus 8dp top/bottom insets.
            // Keep the demo's morph, but use the supplied iOS menu's compact layout.
            val currentWidth = (44.dp + (expandedWidth - 44.dp) * progress)
                .coerceIn(44.dp, (maxWidth - 12.dp).coerceAtLeast(44.dp))
            val currentHeight = (44.dp + (expandedHeight - 44.dp) * progress)
                .coerceIn(44.dp, (maxHeight - 12.dp).coerceAtLeast(44.dp))
            val currentShape = remember(progress, currentWidth, currentHeight) {
                val maxRadius = minOf(currentWidth, currentHeight) / 2f
                val radius = maxRadius + (24.dp - maxRadius) * progress.coerceIn(0f, 1f)
                RoundedRectangle(radius, RoundedCornerStyle.Continuous)
            }
            val widthPx = with(density) { currentWidth.roundToPx() }
            val heightPx = with(density) { currentHeight.roundToPx() }
            val windowWidth = with(density) { maxWidth.roundToPx() }
            val windowHeight = with(density) { maxHeight.roundToPx() }
            val growRight = anchor.left + anchor.width / 2 < windowWidth / 2
            val panelX = (if (growRight) anchor.left else anchor.right - widthPx)
                .coerceIn(0, (windowWidth - widthPx).coerceAtLeast(0))
            val panelY = (if (anchor.top + heightPx <= windowHeight) anchor.top else anchor.bottom - heightPx)
                .coerceIn(0, (windowHeight - heightPx).coerceAtLeast(0))
            val panelOffset = IntOffset(panelX, panelY)
            SideEffect { onPosition(panelOffset) }
            Box(
                Modifier.offset { panelOffset }
                    .size(currentWidth, currentHeight)
                    .clickable(onClickLabel = "收起空间音频菜单") { isOpen = false }
                    .drawBackdrop(
                        backdrop = backdrop,
                        shape = { currentShape },
                        effects = {
                            vibrancy()
                            blur(12.dp.toPx())
                            lens(refractionHeight = 24.dp.toPx(), refractionAmount = 32.dp.toPx(), chromaticAberration = true)
                        },
                        onDrawSurface = {
                            if (!hasBackdrop) drawRect(if (dark) Color(0xFF242426) else Color(0xFFE8E8ED))
                            drawRect(Color.White.copy(alpha = 0.2f))
                            drawRect(Color.White.copy(alpha = 0.4f), style = Stroke(width = 1.dp.toPx()))
                        }
                    )
            ) {
                if (progress > 0.1f) {
                    Column(
                        Modifier.fillMaxSize().padding(vertical = 8.dp).selectableGroup(),
                        verticalArrangement = Arrangement.Top,
                        horizontalAlignment = Alignment.Start
                    ) {
                        val contentAlpha = ((progress - 0.2f).coerceAtLeast(0f) / 0.8f).coerceAtMost(1f)
                        @Composable
                        fun MenuButton(text: String, value: Boolean) {
                            Row(
                                Modifier.fillMaxWidth().height(44.dp).padding(horizontal = 16.dp)
                                    .graphicsLayer { alpha = contentAlpha }
                                    .selectable(selected = enabled == value, enabled = isOpen && progress > .8f,
                                        role = Role.RadioButton) {
                                        isOpen = false
                                        onSelect(value)
                                    },
                                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.Start),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                val itemColor = if (dark) Color.White else Color.Black
                                Box(Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                                    if (enabled == value) Icon(Icons.Default.Check, null, Modifier.size(20.dp), tint = itemColor)
                                }
                                Text(text, fontSize = 17.sp,
                                    fontWeight = androidx.compose.ui.text.font.FontWeight.Normal, color = itemColor)
                            }
                        }
                        options.forEach { (value, text) -> MenuButton(text, value) }
                    }
                }
                if (progress < .2f) {
                    Icon(Icons.Default.AccountCircle, null,
                        Modifier.align(Alignment.TopEnd).padding(11.dp).size(22.dp)
                            .graphicsLayer { alpha = (1f - progress * 5f).coerceIn(0f, 1f) },
                        tint = if (enabled) accent else if (dark) Color.White else Color(0xFF1C1C1E))
                }
            }
        }
    }
}
