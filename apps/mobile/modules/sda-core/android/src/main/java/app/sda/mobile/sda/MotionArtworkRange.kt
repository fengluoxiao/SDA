package app.sda.mobile.sda

import java.io.RandomAccessFile
import org.json.JSONObject

data class MotionArtworkRange(val offset: Long,val length: Long) {
 companion object {
  private val identifier=byteArrayOf(0x0c,0x4d,0x65,0xf1.toByte(),0x6c,0x1d,0x4f,0xd3.toByte(),0x9d.toByte(),0x46,0x8a.toByte(),0xbd.toByte(),0xb4.toByte(),0x82.toByte(),0x93.toByte(),0x4e)
  fun find(file: RandomAccessFile): MotionArtworkRange? {
   val size=file.length();var at=0L
   repeat(4096) {
    if(at<0 || at>size-8)return null
    file.seek(at);var length=file.readInt().toLong() and 0xffffffffL;val type=file.readInt();var header=8L
    if(length==1L){if(at>size-16)return null;length=file.readLong();header=16}
    if(length==0L)length=size-at
    if(length<header || length>size-at)return null
    if(type==0x75756964 && length>=header+20) {
     val id=ByteArray(16);file.readFully(id);val jsonSize=file.readInt().toLong() and 0xffffffffL
     if(id.contentEquals(identifier) && jsonSize in 1..16384 && jsonSize<=length-header-20) {
      val data=ByteArray(jsonSize.toInt());file.readFully(data)
      val meta=try {JSONObject(String(data,Charsets.UTF_8))}catch(_:Exception){null}
      if(meta?.optString("schema")=="amdl-motion/1" && meta.optString("layout")=="tall" && meta.optString("mime")=="video/mp4") {
       val start=at+header+20+jsonSize;val remaining=length-header-20-jsonSize;val declared=meta.opt("length")
       if(remaining in 8..(64L*1024*1024) && declared is Number && declared.toDouble()==remaining.toDouble()) {
        file.seek(start+4);if(file.readInt()==0x66747970)return MotionArtworkRange(start,remaining)
       }
      }
     }
    }
    at+=length
    if(at==size)return null
   }
   return null
  }
 }
}
