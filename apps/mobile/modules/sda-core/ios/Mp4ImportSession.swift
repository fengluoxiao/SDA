import Foundation

/// One reader/writer pair for one import. Only bounded file blocks cross JS.
final class Mp4ImportSession {
 let source:URL;let output:URL;let size:UInt64
 private var reader:FileHandle?;private var writer:FileHandle?
 private var readOffset:UInt64=0
 init(source:URL,output:URL) throws {
  self.source=source;self.output=output
  size=(try FileManager.default.attributesOfItem(atPath:source.path)[.size] as? NSNumber)?.uint64Value ?? 0
  guard FileManager.default.createFile(atPath:output.path,contents:Data()) else {throw SdaError.message("无法创建提取文件")}
  do {reader=try FileHandle(forReadingFrom:source);writer=try FileHandle(forWritingTo:output)}
  catch {discard();throw error}
 }
 func read(offset:Double,count:Int) throws -> String {
  guard let reader,offset.isFinite,offset>=0,offset.rounded(.towardZero)==offset,offset<=Double(size),count>0,count<=1024*1024 else {throw SdaError.message("导入参数无效")}
  let at=UInt64(offset);if at != readOffset {try reader.seek(toOffset:at)}
  let data=try reader.read(upToCount:count) ?? Data();readOffset=at+UInt64(data.count)
  return data.base64EncodedString()
 }
 func append(_ bytes:String) throws {
  guard let writer,let data=Data(base64Encoded:bytes),data.count<=1024*1024 else {throw SdaError.message("导入参数无效")}
  try writer.write(contentsOf:data)
 }
 func finish() throws -> String {
  try writer?.close();writer=nil;try reader?.close();reader=nil
  let bytes=(try FileManager.default.attributesOfItem(atPath:output.path)[.size] as? NSNumber)?.uint64Value ?? 0
  guard bytes>0 else {throw SdaError.message("提取音轨没有音频帧")}
  return output.absoluteString
 }
 func discard(){try? writer?.close();writer=nil;try? reader?.close();reader=nil;try? FileManager.default.removeItem(at:output)}
 deinit {discard()}
}
