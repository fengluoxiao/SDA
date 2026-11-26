package app.sda.mobile.sda

import android.graphics.Bitmap
import android.media.MediaMetadataRetriever
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max
import kotlin.math.min

class MotionArtworkReverseFrames(private val path:String,private val range:MotionArtworkRange) {
 private val executor=Executors.newSingleThreadExecutor {task->Thread(task,"SDA-motion-reverse").apply{isDaemon=true}}
 private val main=Handler(Looper.getMainLooper())
 private val closed=AtomicBoolean(false)
 private val pending=AtomicBoolean(false)
 private var retriever:MediaMetadataRetriever?=null
 fun request(positionUs:Long,videoWidth:Int,videoHeight:Int,maximumDimension:Int,receive:(Bitmap?)->Unit) {
  if(closed.get() || !pending.compareAndSet(false,true))return
  executor.execute {
   var frame:Bitmap?=null
   try {
    if(!closed.get()) {
     val reader=retriever ?: MediaMetadataRetriever().also {next->
      try {RandomAccessFile(File(path),"r").use {next.setDataSource(it.fd,range.offset,range.length)}}catch(error:Exception){next.release();throw error}
      retriever=next
     }
     val width=max(1,videoWidth);val height=max(1,videoHeight);val scale=min(1.0,maximumDimension.toDouble()/max(width,height))
     val targetWidth=max(1,(width*scale).toInt());val targetHeight=max(1,(height*scale).toInt())
     if(Build.VERSION.SDK_INT>=27)frame=reader.getScaledFrameAtTime(positionUs,MediaMetadataRetriever.OPTION_CLOSEST,targetWidth,targetHeight)
     else {
      val original=reader.getFrameAtTime(positionUs,MediaMetadataRetriever.OPTION_CLOSEST)
      if(original!=null){frame=Bitmap.createScaledBitmap(original,targetWidth,targetHeight,true);if(frame!==original)original.recycle()}
     }
    }
   }catch(_:Exception){}finally{pending.set(false)}
   val result=frame
   main.post {if(closed.get())result?.recycle()else receive(result)}
  }
 }
 fun close(){
  if(!closed.compareAndSet(false,true))return
  executor.execute {retriever?.release();retriever=null}
  executor.shutdown()
 }
}
