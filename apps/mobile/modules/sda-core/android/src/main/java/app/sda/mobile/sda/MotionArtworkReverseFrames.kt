package app.sda.mobile.sda

import android.graphics.Bitmap
import android.media.MediaMetadataRetriever
import android.os.Build
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.Executors
import kotlin.math.max
import kotlin.math.min

class MotionArtworkReverseFrames private constructor(private val path:String,private val range:MotionArtworkRange) {
 companion object {
  private val WINDOW_BYTES=(Runtime.getRuntime().maxMemory()/12).coerceIn(8L*1024*1024,32L*1024*1024)
  private val registry=mutableMapOf<String,MotionArtworkReverseFrames>()
  fun acquire(path:String,range:MotionArtworkRange):MotionArtworkReverseFrames=synchronized(registry) {
   val key="$path:${range.offset}:${range.length}"
   registry[key]?.also{it.users++} ?: MotionArtworkReverseFrames(path,range).also{registry[key]=it}
  }
 }
 private data class Frame(val index:Int,val sharp:Bitmap,val background:Bitmap,val bytes:Long)
 private data class Configuration(val width:Int,val height:Int,val count:Int,val durationUs:Long,val windowFrames:Int,val batchSupported:Boolean) {
  val lastWindow get()=(count-1)/windowFrames
  fun indexAt(timeUs:Long)=((timeUs.toDouble()/durationUs)*count).toInt().coerceIn(0,count-1)
  fun timeAt(index:Int)=(index.toDouble()*durationUs/count).toLong()
 }
 private val executor=Executors.newSingleThreadExecutor {task->Thread(task,"SDA-motion-reverse").apply{isDaemon=true}}
 private val lock=Any()
 private var users=1
 private var closed=false
 private var working=false
 private var failed=false
 private var revision=0
 private var requestedUs:Long?=null
 private var wantsTail=false
 private var wantedWindow:Int?=null
 private var inputWidth=0
 private var inputHeight=0
 private var configuration:Configuration?=null
 private val windows=mutableMapOf<Int,List<Frame>>()
 private var retriever:MediaMetadataRetriever?=null
 private val activeViews=mutableSetOf<Any>()
 fun setActive(view:Any,active:Boolean){synchronized(lock){
  if(active)activeViews.add(view)else if(activeViews.remove(view) && activeViews.isEmpty)reset()
 }}
 fun prepareTail(videoWidth:Int,videoHeight:Int){synchronized(lock){
  if(closed)return
  inputWidth=videoWidth;inputHeight=videoHeight;requestedUs=null;wantsTail=true
  configuration?.let{wantedWindow=it.lastWindow;pruneLocked()};scheduleLocked()
 }}
 fun request(positionUs:Long,videoWidth:Int,videoHeight:Int,maximumDimension:Int,receive:(Bitmap?)->Unit) {
  var error=false
  val frame=synchronized(lock){
   if(closed)return
   inputWidth=videoWidth;inputHeight=videoHeight;requestedUs=positionUs;error=failed
   val config=configuration
   if(config==null){scheduleLocked();null}else{
    val index=config.indexAt(positionUs);val window=index/config.windowFrames
    val wanted=wantedWindow
    if(wanted==null || window<wanted || window>wanted+1 || (window>wanted && !windows.containsKey(window))){wantedWindow=window;pruneLocked()}
    scheduleLocked()
    windows[window]?.minByOrNull{kotlin.math.abs(it.index-index)}
   }
  }
  if(frame!=null)receive(if(maximumDimension<=384)frame.background else frame.sharp)else if(error)receive(null)
 }
 private fun pruneLocked(){val wanted=wantedWindow;windows.keys.removeAll{wanted==null || kotlin.math.abs(it-wanted)>1}}
 private fun scheduleLocked(){
  if(closed || working || failed)return
  val wanted=wantedWindow
  if(configuration!=null && (wanted==null || listOf(wanted,wanted-1).none{it>=0 && !windows.containsKey(it)}))return
  working=true;executor.execute{decodeWindows()}
 }
 private fun loadConfiguration():Configuration {
  val reader=retriever ?: MediaMetadataRetriever().also {next->
   try{RandomAccessFile(File(path),"r").use{next.setDataSource(it.fd,range.offset,range.length)}}catch(error:Exception){next.release();throw error};retriever=next
  }
  val width=reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: max(1,inputWidth)
  val height=reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: max(1,inputHeight)
  require(width in 1..16384 && height in 1..16384)
  val durationUs=(reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0)*1000
  require(durationUs>0)
  val declared=if(Build.VERSION.SDK_INT>=28)reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT)?.toIntOrNull() else null
  val count=declared?.takeIf{it>0} ?: max(1,(durationUs/1000000.0*30).toInt())
  val rawBytes=width.toLong()*height*4
  return Configuration(width,height,count,durationUs,(WINDOW_BYTES/max(1L,rawBytes)).coerceIn(1,24).toInt(),Build.VERSION.SDK_INT>=28 && declared!=null && declared>0)
 }
 private fun decodeWindows(){
  try{
   if(configuration==null){val config=loadConfiguration();synchronized(lock){if(closed){working=false;return};configuration=config;wantedWindow=requestedUs?.let{config.indexAt(it)/config.windowFrames} ?: if(wantsTail)config.lastWindow else null}}
   while(true){
    val task=synchronized(lock){
     val wanted=wantedWindow;val config=configuration
     val window=if(wanted==null)null else listOf(wanted,wanted-1).firstOrNull{it>=0 && !windows.containsKey(it)}
     if(closed || config==null || window==null){working=false;return}
     Triple(config,window,revision)
    }
    val frames=decode(task.first,task.second,task.third)
    synchronized(lock){val wanted=wantedWindow;if(!closed && revision==task.third && wanted!=null && kotlin.math.abs(task.second-wanted)<=1){windows[task.second]=frames;pruneLocked()}}
   }
  }catch(_:Exception){synchronized(lock){if(!closed)failed=true;working=false}}
 }
 private fun scaled(bitmap:Bitmap,maximum:Int):Bitmap {
  val scale=min(1.0,maximum.toDouble()/max(bitmap.width,bitmap.height))
  return Bitmap.createScaledBitmap(bitmap,max(1,(bitmap.width*scale).toInt()),max(1,(bitmap.height*scale).toInt()),true)
 }
 private fun decode(config:Configuration,window:Int,token:Int):List<Frame>{
  val reader=retriever ?: return emptyList()
  val start=window*config.windowFrames;val count=min(config.windowFrames,config.count-start)
  val originals=if(config.batchSupported && Build.VERSION.SDK_INT>=28)reader.getFramesAtIndex(start,count) else (0 until count).map{index->
   val position=config.timeAt(start+index)
   (if(Build.VERSION.SDK_INT>=27){val scale=min(1.0,1024.0/max(config.width,config.height));reader.getScaledFrameAtTime(position,MediaMetadataRetriever.OPTION_CLOSEST,max(1,(config.width*scale).toInt()),max(1,(config.height*scale).toInt()))}else reader.getFrameAtTime(position,MediaMetadataRetriever.OPTION_CLOSEST)) ?: throw IllegalStateException("Missing reverse frame")
  }
  check(originals.isNotEmpty())
  val frames=mutableListOf<Frame>();var bytes=0L
  for((offset,original)in originals.withIndex()){
   if(synchronized(lock){closed || revision!=token})break
   val sharp=scaled(original,1024);val background=scaled(original,384)
   val frameBytes=sharp.rowBytes.toLong()*sharp.height+background.rowBytes.toLong()*background.height
   if(bytes+frameBytes>WINDOW_BYTES)break
   frames.add(Frame(start+offset,sharp,background,frameBytes));bytes+=frameBytes
   if(original!==sharp && original!==background)original.recycle()
  }
  return frames
 }
 fun reset(){synchronized(lock){revision++;windows.clear();requestedUs=null;wantsTail=false;wantedWindow=null}}
 fun release(){
  val finished=synchronized(registry){users--;if(users==0){registry.remove("$path:${range.offset}:${range.length}");true}else false}
  if(!finished)return
  synchronized(lock){closed=true;revision++;windows.clear()}
  executor.execute{retriever?.release();retriever=null};executor.shutdown()
 }
}
