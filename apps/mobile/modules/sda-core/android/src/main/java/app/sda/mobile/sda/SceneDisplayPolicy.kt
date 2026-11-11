package app.sda.mobile.sda

import android.app.Activity
import android.os.Build
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/** A visible scene may request a refresh mode, never a different resolution. */
internal class SceneDisplayPolicy {
    private data class Saved(val mode: Int, val rate: Float, val navigation: Boolean, val behavior: Int, val cutout: Int, val viewRate: Float)
    private var owner: Activity? = null
    private var saved: Saved? = null
    private var fullscreen = false
    private fun modes(activity: Activity): List<android.view.Display.Mode> {
        val display = if (Build.VERSION.SDK_INT >= 30) activity.display else @Suppress("DEPRECATION") activity.windowManager.defaultDisplay
        val current = display?.mode ?: return emptyList()
        return display.supportedModes.filter { it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight }
    }
    fun capabilities(activity: Activity?): Map<String, Any> = mapOf("maximumFps" to (activity?.let { modes(it).maxOfOrNull { mode -> mode.refreshRate } } ?: 60f))
    fun configure(activity: Activity?, active: Boolean, requestedFps: Int, full: Boolean) {
        activity ?: return
        activity.runOnUiThread {
            if (owner != null && owner !== activity) restore()
            val window = activity.window
            val controller = WindowCompat.getInsetsController(window, window.decorView)
            if (!active) { restore(); return@runOnUiThread }
            if (saved == null) {
                val insets = ViewCompat.getRootWindowInsets(window.decorView)
                saved = Saved(window.attributes.preferredDisplayModeId, window.attributes.preferredRefreshRate,
                    insets?.isVisible(WindowInsetsCompat.Type.navigationBars()) ?: true, controller.systemBarsBehavior, if (Build.VERSION.SDK_INT >= 28) window.attributes.layoutInDisplayCutoutMode else 0, if (Build.VERSION.SDK_INT >= 35) window.decorView.requestedFrameRate else 0f)
                owner = activity
            }
            val supported = modes(activity)
            val fps = if (requestedFps == 120 && supported.any { it.refreshRate >= 119.5f }) 120f else 60f
            val chosen = supported.filter { kotlin.math.abs(it.refreshRate - fps) < .6f }.minByOrNull { kotlin.math.abs(it.refreshRate - fps) }
            window.attributes = window.attributes.apply { preferredDisplayModeId = chosen?.modeId ?: 0; preferredRefreshRate = fps
                if (Build.VERSION.SDK_INT >= 28) layoutInDisplayCutoutMode = if(full) android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES else saved?.cutout ?: layoutInDisplayCutoutMode
            }
            if (Build.VERSION.SDK_INT >= 35) window.decorView.requestedFrameRate = fps
            if (full) {
                controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                controller.hide(WindowInsetsCompat.Type.navigationBars())
            } else if (fullscreen) restoreBars(controller)
            fullscreen = full
        }
    }
    private fun restoreBars(controller: WindowInsetsControllerCompat) {
        val previous = saved ?: return
        controller.systemBarsBehavior = previous.behavior
        // React Native StatusBar owns status visibility. Restoring a captured
        // status flag here races its fullscreen exit update on the UI thread.
        val navigation = WindowInsetsCompat.Type.navigationBars()
        if (previous.navigation) controller.show(navigation) else controller.hide(navigation)
    }
    fun restore() {
        val activity = owner ?: return
        val previous = saved ?: return
        activity.window.attributes = activity.window.attributes.apply { preferredDisplayModeId = previous.mode; preferredRefreshRate = previous.rate
            if (Build.VERSION.SDK_INT >= 28) layoutInDisplayCutoutMode = previous.cutout
        }
        if (Build.VERSION.SDK_INT >= 35) activity.window.decorView.requestedFrameRate = previous.viewRate
        if (fullscreen) restoreBars(WindowCompat.getInsetsController(activity.window, activity.window.decorView))
        saved = null; owner = null; fullscreen = false
    }
}
