package com.kyant.backdrop.catalog.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

// Shared neutral material for navigation and its settings/back controls.
internal val LiquidGlassBlurRadius = 8.dp
internal fun liquidGlassSurfaceColor(dark: Boolean): Color =
    (if (dark) Color(0xFF121212) else Color(0xFFFAFAFA)).copy(alpha = 0.4f)
