package com.kyant.backdrop.catalog.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.unit.dp

// Shared neutral material for navigation and its settings/back controls.
internal val LiquidGlassBlurRadius = 8.dp
internal fun liquidGlassSurfaceColor(dark: Boolean): Color =
    (if (dark) Color(0xFF121212) else Color(0xFFFAFAFA)).copy(alpha = 0.4f)


// Paint this under captured foreground content when its original theme color
// must be preserved. The visible neutral selection pill stays the same.
internal fun DrawScope.drawLiquidTabSelection(isLightTheme: Boolean, progress: Float) {
    drawRect(if (isLightTheme) Color.Black.copy(alpha = .1f) else Color.White.copy(alpha = .1f), alpha = 1f - progress)
    drawRect(Color.Black.copy(alpha = .03f * progress))
}
