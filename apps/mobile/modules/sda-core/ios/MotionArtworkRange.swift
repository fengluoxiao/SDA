import Foundation
import CoreFoundation

// AMDL UUID artwork is independent of covr and audio sample tables. Only the
// requested portrait variant is exposed, without writing another media file.
struct MotionArtworkRange {
 let offset: UInt64, length: UInt64
 static let maximumBytes: UInt64 = 64 * 1024 * 1024
 private static let identifier = Data([0x0c,0x4d,0x65,0xf1,0x6c,0x1d,0x4f,0xd3,0x9d,0x46,0x8a,0xbd,0xb4,0x82,0x93,0x4e])
 static func find(in file: FileHandle, size: UInt64) throws -> MotionArtworkRange? {
  func bytes(_ at: UInt64,_ count: Int) throws -> Data {
   guard at <= size, UInt64(count) <= size-at else {return Data()}
   try file.seek(toOffset:at);return try file.read(upToCount:count) ?? Data()
  }
  func number(_ data: Data,_ from: Int,_ count: Int) -> UInt64 {
   data[from..<from+count].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
  }
  var at: UInt64 = 0
  for _ in 0..<4096 {
   let head=try bytes(at,8);guard head.count==8 else {return nil}
   var length=number(head,0,4), header:UInt64=8
   if length==1 {let wide=try bytes(at+8,8);guard wide.count==8 else{return nil};length=number(wide,0,8);header=16}
   if length==0 {length=size-at}
   guard length>=header, length<=size-at else {return nil}
   if head[4..<8]==Data("uuid".utf8),length>=header+20 {
    let prefix=try bytes(at+header,20)
    if prefix.count==20, prefix.prefix(16)==identifier {
     let jsonSize=number(prefix,16,4)
     if jsonSize>0,jsonSize<=16384,jsonSize<=length-header-20,
        let object=try? JSONSerialization.jsonObject(with:bytes(at+header+20,Int(jsonSize))) as? [String:Any],
        object["schema"] as? String == "amdl-motion/1",object["layout"] as? String == "tall",object["mime"] as? String == "video/mp4",
        let declared=object["length"] as? NSNumber, CFGetTypeID(declared) != CFBooleanGetTypeID() {
      let start=at+header+20+jsonSize, remaining=length-header-20-jsonSize
      if remaining>=8,remaining<=maximumBytes,declared.doubleValue==Double(remaining) {
       let signature=try bytes(start,8)
       if signature.count==8,signature[4..<8]==Data("ftyp".utf8) {return .init(offset:start,length:remaining)}
      }
     }
    }
   }
   at+=length
   if at==size {return nil}
  }
  return nil
 }
}
