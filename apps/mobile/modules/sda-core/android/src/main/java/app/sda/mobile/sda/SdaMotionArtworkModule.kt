package app.sda.mobile.sda

import android.content.Context
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.media.MediaPlayer
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
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
  View(SdaMotionArtworkView::class) {
   Events("onStatus")
   Prop("sourceUri") {view:SdaMotionArtworkView,value:String->view.updateSource(value)}
   Prop("active") {view:SdaMotionArtworkView,value:Boolean->view.updateActive(value)}
  }
 }
}

class SdaMotionArtworkView(context: Context,appContext: AppContext): ExpoView(context,appContext),TextureView.SurfaceTextureListener,LifecycleEventListener {
 override val shouldUseAndroidLayout=true
 private val onStatus by EventDispatcher()
 private val react=appContext.reactContext as? ReactContext
 private val main=Handler(Looper.getMainLooper())
 private val texture=TextureView(context)
 private var source=""
 private var active=false
 private var foreground=true
 @Volatile private var generation=0
 private var starting=false
 private var player:MediaPlayer?=null
 private var surface:Surface?=null
 private var videoWidth=0
 private var videoHeight=0
 init {addView(texture,ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));texture.surfaceTextureListener=this;texture.isOpaque=false;isClickable=false;clipChildren=true}
 fun updateSource(value:String){if(source!=value){source=value;stop();startIfNeeded()}}
 fun updateActive(value:Boolean){active=value;if(value)startIfNeeded()else player?.pause()}
 override fun onAttachedToWindow(){super.onAttachedToWindow();react?.addLifecycleEventListener(this);startIfNeeded()}
 override fun onDetachedFromWindow(){react?.removeLifecycleEventListener(this);stop();super.onDetachedFromWindow()}
 override fun onWindowVisibilityChanged(visibility:Int){super.onWindowVisibilityChanged(visibility);if(visibility==View.VISIBLE)startIfNeeded()else stop()}
 override fun onHostResume(){foreground=true;startIfNeeded()}
 override fun onHostPause(){foreground=false;player?.pause()}
 override fun onHostDestroy(){foreground=false;stop()}
 private fun crop(){
  if(videoWidth<=0 || videoHeight<=0 || texture.width<=0 || texture.height<=0)return
  val ratio=videoWidth.toFloat()/videoHeight;val viewport=texture.width.toFloat()/texture.height
  val matrix=Matrix();matrix.setScale(if(ratio>viewport)ratio/viewport else 1f,if(ratio<viewport)viewport/ratio else 1f,texture.width/2f,texture.height/2f);texture.setTransform(matrix)
 }
 private fun startIfNeeded(){
  if(player!=null){if(active && foreground && isAttachedToWindow && windowVisibility==View.VISIBLE)player?.start();return}
  if(!active || !foreground || !isAttachedToWindow || windowVisibility!=View.VISIBLE || !texture.isAvailable || starting)return
  val uri=Uri.parse(source);if(uri.scheme!="file")return
  val path=uri.path ?: return;val token=generation;val current=source;starting=true
  reader.execute {
   if(generation!=token)return@execute
   val range=try {RandomAccessFile(File(path),"r").use {MotionArtworkRange.find(it)}}catch(_:Exception){null}
   main.post {
    if(generation!=token || !active || !foreground || !isAttachedToWindow || !texture.isAvailable)return@post
    starting=false
    if(range==null){onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true));return@post}
    try {
     val output=Surface(texture.surfaceTexture);surface=output
     val next=MediaPlayer();player=next
     next.setSurface(output);next.setVolume(0f,0f);next.isLooping=true
     // Android duplicates this descriptor; retain no extraction or cached clip.
     RandomAccessFile(File(path),"r").use {file->next.setDataSource(file.fd,range.offset,range.length)}
     next.setOnVideoSizeChangedListener {_,w,h->if(generation==token){videoWidth=w;videoHeight=h;crop()}}
     next.setOnPreparedListener {if(generation==token && active && foreground){crop();it.start()}}
     next.setOnInfoListener {_,what,_->if(generation==token && what==MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START)onStatus(mapOf("uri" to current,"ready" to true));false}
     next.setOnErrorListener {_,_,_->if(generation==token){stop();onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true))};true}
     next.prepareAsync()
    }catch(_:Exception){stop();onStatus(mapOf("uri" to current,"ready" to false,"unavailable" to true))}
   }
  }
 }
 private fun stop(){generation++;starting=false;player?.setOnPreparedListener(null);player?.setOnErrorListener(null);player?.setOnInfoListener(null);player?.setOnVideoSizeChangedListener(null);player?.release();player=null;surface?.release();surface=null;videoWidth=0;videoHeight=0}
 override fun onSurfaceTextureAvailable(surface:SurfaceTexture,width:Int,height:Int){startIfNeeded()}
 override fun onSurfaceTextureSizeChanged(surface:SurfaceTexture,width:Int,height:Int){crop()}
 override fun onSurfaceTextureDestroyed(surface:SurfaceTexture):Boolean{stop();return true}
 override fun onSurfaceTextureUpdated(surface:SurfaceTexture){}
 companion object {private val reader=Executors.newSingleThreadExecutor {r->Thread(r,"SDA-motion-artwork").apply{isDaemon=true}}}
}
