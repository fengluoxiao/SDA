package app.sda.mobile.sda

import android.content.Context
import android.graphics.Matrix
import android.graphics.Bitmap
import android.graphics.RenderEffect
import android.graphics.SurfaceTexture
import android.graphics.Shader
import android.media.MediaPlayer
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import com.facebook.react.bridge.LifecycleEventListener
import com.facebook.react.bridge.ReactContext
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.Executors

class SdaMotionArtworkModule: Module() {
 override fun definition()=ModuleDefinition {
  Name("SdaMotionArtwork")
  AsyncFunction("hasMotionArtwork") {uri:String->
   val parsed=Uri.parse(uri)
   if(parsed.scheme!="file" || parsed.path==null)false else try{RandomAccessFile(File(parsed.path!!),"r").use{MotionArtworkRange.find(it)!=null}}catch(_:Exception){false}
  }
  View(SdaMotionArtworkView::class) {
   Events("onStatus")
   Prop("sourceUri") {view:SdaMotionArtworkView,value:String->view.updateSource(value)}
   Prop("active") {view:SdaMotionArtworkView,value:Boolean->view.updateActive(value)}
   Prop("presented") {view:SdaMotionArtworkView,value:Boolean->view.updatePresented(value)}
   Prop("blurred") {view:SdaMotionArtworkView,value:Boolean->view.updateBlurred(value)}
  }
 }
}

class SdaMotionArtworkView(context: Context,appContext: AppContext): ExpoView(context,appContext),TextureView.SurfaceTextureListener,LifecycleEventListener {
 override val shouldUseAndroidLayout=true
 private val onStatus by EventDispatcher()
 private val react=appContext.reactContext as? ReactContext
 private val main=Handler(Looper.getMainLooper())
 private val texture=TextureView(context)
 private val reverseImage=ImageView(context).apply {scaleType=ImageView.ScaleType.CENTER_CROP;visibility=View.INVISIBLE}
 private var source=""
 private var active=false
 private var presented=false
 private var prepared=false
 private var blurred=false
 private var foreground=true
 @Volatile private var generation=0
 private var starting=false
 private var player:MediaPlayer?=null
 private var surface:Surface?=null
 private var videoWidth=0
 private var videoHeight=0
 private var reverseFrames:MotionArtworkReverseFrames?=null
 private var reversing=false
 private var reversePositionMs=0L
 private var reverseStartedNs=0L
 private var reverseEpoch=0
 private var reverseScheduled=false
 private var waitingForwardFrame=false
 private var forwardSeekComplete=false
 private var tailScheduled=false
 private var lastReverseBitmap:Bitmap?=null
 private val reverseTick=Runnable {reverseScheduled=false;renderReverseFrame()}
 private val tailTick=Runnable {tailScheduled=false;prepareReverseTail()}
 init {addView(texture,ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));addView(reverseImage,ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));texture.surfaceTextureListener=this;texture.isOpaque=false;isClickable=false;clipChildren=true}
 fun updateSource(value:String){if(source!=value){source=value;stop();startIfNeeded()}}
 fun updateActive(value:Boolean){active=value;if(value)startIfNeeded()else pause()}
 fun updatePresented(value:Boolean){
  if(presented==value)return
  presented=value
  if(value)startIfNeeded()else rewind()
 }
 fun updateBlurred(value:Boolean){
  blurred=value
  if(android.os.Build.VERSION.SDK_INT>=31){val effect=if(value)RenderEffect.createBlurEffect(36f,36f,Shader.TileMode.DECAL)else null;texture.setRenderEffect(effect);reverseImage.setRenderEffect(effect)}
 }
 override fun onAttachedToWindow(){super.onAttachedToWindow();react?.addLifecycleEventListener(this);startIfNeeded()}
 override fun onDetachedFromWindow(){react?.removeLifecycleEventListener(this);pause();super.onDetachedFromWindow()}
 override fun onWindowVisibilityChanged(visibility:Int){super.onWindowVisibilityChanged(visibility);if(visibility==View.VISIBLE)startIfNeeded()else pause()}
 override fun onHostResume(){foreground=true;startIfNeeded()}
 override fun onHostPause(){foreground=false;pause()}
 override fun onHostDestroy(){foreground=false;stop()}
 private fun crop(){
  if(videoWidth<=0 || videoHeight<=0 || texture.width<=0 || texture.height<=0)return
  val ratio=videoWidth.toFloat()/videoHeight;val viewport=texture.width.toFloat()/texture.height
  val matrix=Matrix();matrix.setScale(if(ratio>viewport)ratio/viewport else 1f,if(ratio<viewport)viewport/ratio else 1f,texture.width/2f,texture.height/2f);texture.setTransform(matrix)
 }
 private fun startIfNeeded(){
  if(player!=null){resume();return}
  if(!foreground || !isAttachedToWindow || windowVisibility!=View.VISIBLE || !texture.isAvailable || starting)return
  val uri=Uri.parse(source);if(uri.scheme!="file")return
  val path=uri.path ?: return;val token=generation;val current=source;starting=true
  reader.execute {
   if(generation!=token)return@execute
   val range=try {RandomAccessFile(File(path),"r").use {MotionArtworkRange.find(it)}}catch(_:Exception){null}
   main.post {
   if(generation!=token || !foreground || !isAttachedToWindow || !texture.isAvailable)return@post
    starting=false
    if(range==null){onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true));return@post}
    try {
     val output=Surface(texture.surfaceTexture);surface=output
     val next=MediaPlayer();player=next
     next.setSurface(output);next.setVolume(0f,0f);next.isLooping=false
     reverseFrames=MotionArtworkReverseFrames.acquire(path,range)
     // Android duplicates this descriptor; retain no extraction or cached clip.
     RandomAccessFile(File(path),"r").use {file->next.setDataSource(file.fd,range.offset,range.length)}
     next.setOnVideoSizeChangedListener {_,w,h->if(generation==token){videoWidth=w;videoHeight=h;crop()}}
     next.setOnPreparedListener {if(generation==token && foreground){prepared=true;crop();it.start()}}
     next.setOnInfoListener {_,what,_->if(generation==token && what==MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START){onStatus(mapOf("uri" to current,"ready" to true));if(!active || !presented)pause()};false}
     next.setOnCompletionListener {if(generation==token)beginReverse()}
     next.setOnSeekCompleteListener {if(generation==token){forwardSeekComplete=true;resume()}}
     next.setOnErrorListener {_,_,_->if(generation==token){stop();onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true))};true}
     next.prepareAsync()
    }catch(_:Exception){stop();onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true))}
   }
  }
 }
 private fun canAnimate()=active && presented && foreground && isAttachedToWindow && windowVisibility==View.VISIBLE && prepared
 private fun currentReversePosition()=maxOf(0L,reversePositionMs-if(reverseStartedNs==0L)0L else (System.nanoTime()-reverseStartedNs)/1000000L)
 private fun resume(){
  if(!canAnimate())return
  if(reversing){if(reverseStartedNs==0L)reverseStartedNs=System.nanoTime();scheduleReverse(0)}else{player?.start();prepareReverseTail()}
 }
 private fun scheduleReverse(delay:Long=16){if(!reverseScheduled && canAnimate()){reverseScheduled=true;main.postDelayed(reverseTick,delay)}}
 private fun prepareReverseTail(){
  if(!canAnimate() || reversing)return
  try{val current=player ?: return;if(current.currentPosition>=current.duration-1500)reverseFrames?.prepareTail(videoWidth,videoHeight)}catch(_:IllegalStateException){return}
  if(!tailScheduled){tailScheduled=true;main.postDelayed(tailTick,200)}
 }
 private fun beginReverse(){
  if(!presented){rewind();return}
  val duration=player?.duration ?: return
  if(duration<=0)return
  reverseFrames?.prepareTail(videoWidth,videoHeight);lastReverseBitmap=null
  reversing=true;reversePositionMs=maxOf(0L,duration.toLong()-1);reverseStartedNs=0L;reverseEpoch++
  resume()
 }
 private fun renderReverseFrame(){
  if(!canAnimate() || !reversing)return
  val position=currentReversePosition()
  if(position<=0){pause();reversing=false;waitingForwardFrame=true;forwardSeekComplete=false;player?.seekTo(0);return}
  val token=generation;val epoch=reverseEpoch
  reverseFrames?.request(position*1000L,videoWidth,videoHeight,if(blurred)384 else 1024){frame->
   if(generation!=token || reverseEpoch!=epoch || !reversing || !canAnimate())return@request
   if(frame==null){val uri=source;stop();onStatus(mapOf("uri" to uri,"ready" to false,"unavailable" to true));return@request}
   if(lastReverseBitmap!==frame){lastReverseBitmap=frame;reverseImage.setImageBitmap(frame);reverseImage.visibility=View.VISIBLE}
  }
  scheduleReverse()
 }
 private fun clearReverseImage(){reverseImage.visibility=View.INVISIBLE;reverseImage.setImageDrawable(null);lastReverseBitmap=null;waitingForwardFrame=false}
 private fun rewind(){pause();reversing=false;reversePositionMs=0;reverseStartedNs=0;reverseFrames?.reset();clearReverseImage();if(prepared){forwardSeekComplete=false;player?.seekTo(0)}}
 private fun stop(){generation++;starting=false;pause();prepared=false;reversing=false;reversePositionMs=0;reverseStartedNs=0;reverseFrames?.release();reverseFrames=null;clearReverseImage();player?.setOnPreparedListener(null);player?.setOnErrorListener(null);player?.setOnInfoListener(null);player?.setOnCompletionListener(null);player?.setOnSeekCompleteListener(null);player?.setOnVideoSizeChangedListener(null);player?.release();player=null;surface?.release();surface=null;videoWidth=0;videoHeight=0}
 private fun pause(){if(reversing){reversePositionMs=currentReversePosition();reverseStartedNs=0};reverseEpoch++;main.removeCallbacks(reverseTick);main.removeCallbacks(tailTick);tailScheduled=false;reverseScheduled=false;if(prepared)try{player?.pause()}catch(_:IllegalStateException){}}
 override fun onSurfaceTextureAvailable(surface:SurfaceTexture,width:Int,height:Int){startIfNeeded()}
 override fun onSurfaceTextureSizeChanged(surface:SurfaceTexture,width:Int,height:Int){crop()}
 override fun onSurfaceTextureDestroyed(surface:SurfaceTexture):Boolean{stop();return true}
 override fun onSurfaceTextureUpdated(surface:SurfaceTexture){if(waitingForwardFrame && forwardSeekComplete)clearReverseImage()}
 companion object {private val reader=Executors.newSingleThreadExecutor {r->Thread(r,"SDA-motion-artwork").apply{isDaemon=true}}}
}
