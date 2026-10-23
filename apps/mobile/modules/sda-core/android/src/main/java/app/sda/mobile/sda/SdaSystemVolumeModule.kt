package app.sda.mobile.sda

import android.content.Context
import android.database.ContentObserver
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.drawable.ColorDrawable
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.View
import android.widget.SeekBar
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView

class SdaSystemVolumeModule : Module() {
 override fun definition() = ModuleDefinition {
  Name("SdaSystemVolume")
  View(SdaSystemVolumeView::class) {
   Events("onChange", "onTracking")
   Prop("activeColorArgb") { view: SdaSystemVolumeView, color: Long -> view.slider.activeColor = color.toInt(); view.slider.invalidate() }
   Prop("inactiveColorArgb") { view: SdaSystemVolumeView, color: Long -> view.slider.inactiveColor = color.toInt(); view.slider.invalidate() }
  }
 }
}

class MediaVolumeSeekBar(context: Context) : SeekBar(context) {
 var activeColor = Color.rgb(201, 201, 206)
 var inactiveColor = Color.argb(61, 255, 255, 255)
 private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
 private val track = RectF()
 init {
  setPadding(0, 0, 0, 0)
  thumb = ColorDrawable(Color.TRANSPARENT)
  splitTrack = false
  contentDescription = "媒体音量"
 }
 override fun onDraw(canvas: Canvas) {
  val thickness = 5 * resources.displayMetrics.density
  val top = (height - thickness) / 2
  track.set(0f, top, width.toFloat(), top + thickness)
  paint.color = inactiveColor
  canvas.drawRoundRect(track, thickness / 2, thickness / 2, paint)
  if (max <= 0 || progress <= 0) return
  val fraction = progress.toFloat() / max
  if (layoutDirection == View.LAYOUT_DIRECTION_RTL) track.left = width * (1 - fraction) else track.right = width * fraction
  paint.color = activeColor
  canvas.drawRoundRect(track, thickness / 2, thickness / 2, paint)
 }
}

class SdaSystemVolumeView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
 override val shouldUseAndroidLayout = true
 val slider = MediaVolumeSeekBar(context)
 private val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
 private val main = Handler(Looper.getMainLooper())
 private val onChange by EventDispatcher()
 private val onTracking by EventDispatcher()
 private var observing = false
 private var minimum = 0
 private var lastVolume: Int? = null
 private var lastMaximum: Int? = null
 private val observer = object : ContentObserver(main) { override fun onChange(selfChange: Boolean) = refresh() }
 private val devices = object : AudioDeviceCallback() {
  override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) = refresh()
  override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) = refresh()
 }
 init {
  addView(slider, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
  slider.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
   override fun onProgressChanged(seekBar: SeekBar, progress: Int, fromUser: Boolean) {
    if (!fromUser || !slider.isEnabled) return
    // System volume is written only for a user's interaction, never on mount,
    // configuration changes, observer notifications, or route restoration.
    runCatching { audio.setStreamVolume(AudioManager.STREAM_MUSIC, minimum + progress, 0) }
    refresh()
   }
   override fun onStartTrackingTouch(seekBar: SeekBar) { parent?.requestDisallowInterceptTouchEvent(true); onTracking(mapOf("active" to true)) }
   override fun onStopTrackingTouch(seekBar: SeekBar) { parent?.requestDisallowInterceptTouchEvent(false); onTracking(mapOf("active" to false)); refresh() }
  })
 }
 override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
  super.onLayout(changed, left, top, right, bottom)
  slider.layout(0, 0, right - left, bottom - top)
 }
 override fun onAttachedToWindow() { super.onAttachedToWindow(); updateObservation() }
 override fun onDetachedFromWindow() { stopObserving(); super.onDetachedFromWindow() }
 override fun onWindowVisibilityChanged(visibility: Int) { super.onWindowVisibilityChanged(visibility); updateObservation() }
 private fun updateObservation() {
  if (!isAttachedToWindow || windowVisibility != View.VISIBLE) { stopObserving(); return }
  if (!observing) {
   context.contentResolver.registerContentObserver(Settings.System.CONTENT_URI, true, observer)
   audio.registerAudioDeviceCallback(devices, main)
   observing = true
  }
  refresh()
 }
 private fun stopObserving() {
  if (observing) { context.contentResolver.unregisterContentObserver(observer); audio.unregisterAudioDeviceCallback(devices) }
  observing = false; lastVolume = null; lastMaximum = null
 }
 private fun refresh() {
  if (!isAttachedToWindow || windowVisibility != View.VISIBLE) return
  val maximum = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
  minimum = if (Build.VERSION.SDK_INT >= 28) audio.getStreamMinVolume(AudioManager.STREAM_MUSIC) else 0
  val volume = audio.getStreamVolume(AudioManager.STREAM_MUSIC).coerceIn(minimum, maximum.coerceAtLeast(minimum))
  slider.isEnabled = !audio.isVolumeFixed && maximum > minimum
  slider.max = (maximum - minimum).coerceAtLeast(0)
  slider.progress = volume - minimum
  if (volume != lastVolume || maximum != lastMaximum) {
   lastVolume = volume; lastMaximum = maximum
   onChange(mapOf("value" to if (maximum > minimum) (volume - minimum).toDouble() / (maximum - minimum) else 0.0))
  }
 }
}
