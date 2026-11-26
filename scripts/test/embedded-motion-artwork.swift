import Foundation
import AVFoundation
import CryptoKit

@main struct MotionArtworkTest {
 static func require(_ test:Bool,_ message:String) throws {if !test{throw NSError(domain:message,code:1)}}
 static func main() async throws {
  var reverseClock=MotionArtworkReverseClock()
  reverseClock.begin(at:5);reverseClock.resume(at:100)
  try require(reverseClock.current(at:102)==3,"Reverse playback must move from five to three seconds")
  reverseClock.pause(at:102)
  try require(reverseClock.current(at:200)==3,"Paused reverse playback must retain its position")
  reverseClock.resume(at:200);reverseClock.resume(at:200.5)
  try require(reverseClock.current(at:201)==2,"Resume must not restart the reverse leg")
  try require(reverseClock.current(at:210)==0,"Reverse playback must stop at zero")
  reverseClock.begin(at:0)
  try require(reverseClock.current(at:300)==0,"Track switches must reset the reverse clock")
  let args=CommandLine.arguments;try require(args.count==3,"Pass synthetic clip and fresh test directory")
  let video=try Data(contentsOf:URL(fileURLWithPath:args[1])),root=URL(fileURLWithPath:args[2],isDirectory:true)
  try require(!FileManager.default.fileExists(atPath:root.path),"Fresh directory required");try FileManager.default.createDirectory(at:root,withIntermediateDirectories:false)
  func be(_ value:UInt32)->Data {var n=value.bigEndian;return withUnsafeBytes(of:&n){Data($0)}}
  func atom(_ kind:String,_ body:Data)->Data {be(UInt32(body.count+8))+Data(kind.utf8)+body}
  let uuid=Data([0x0c,0x4d,0x65,0xf1,0x6c,0x1d,0x4f,0xd3,0x9d,0x46,0x8a,0xbd,0xb4,0x82,0x93,0x4e])
  func motion(_ layout:String,_ clip:Data,_ schema:String="amdl-motion/1",_ declared:Any?=nil,_ id:Data?=nil)->Data {
   let meta:[String:Any]=["schema":schema,"layout":layout,"default":layout=="square","mime":"video/mp4","loop":true,"length":declared ?? clip.count,"sha256":SHA256.hash(data:clip).map{String(format:"%02x",$0)}.joined()]
   let header=try! JSONSerialization.data(withJSONObject:meta,options:.sortedKeys)
   return atom("uuid",(id ?? uuid)+be(UInt32(header.count))+header+clip)
  }
  let prefix=atom("ftyp",Data("M4A \0\0\0\0".utf8))+atom("mdat",Data(repeating:0,count:81))
  let square=motion("square",Data([0,0,0,8])+Data("ftyp".utf8))
  func save(_ name:String,_ bytes:Data)throws->URL {let file=root.appendingPathComponent(name);try bytes.write(to:file);return file}
  func read(_ file:URL)throws->MotionArtworkRange?{let handle=try FileHandle(forReadingFrom:file);defer{try? handle.close()};return try MotionArtworkRange.find(in:handle,size:handle.seekToEnd())}
  let parent=try save("embedded.m4a",prefix+square+motion("tall",video))
  guard let range=try read(parent),range.length==UInt64(video.count) else {throw NSError(domain:"Portrait variant was not selected",code:1)}
  let handle=try FileHandle(forReadingFrom:parent);try handle.seek(toOffset:range.offset);let actual=try handle.read(upToCount:Int(range.length));try handle.close()
  try require(actual==video,"Range changed the embedded video")
  let badSchema=prefix+motion("tall",video,"other/1")
  let badLength=prefix+motion("tall",video,"amdl-motion/1",true)
  let wrongUUID=prefix+motion("tall",video,"amdl-motion/1",nil,Data(repeating:1,count:16))
  let completeTall=prefix+motion("tall",video)
  let truncated=completeTall.dropLast(1).withUnsafeBytes{Data($0)}
  let badExtended=be(1)+Data("uuid".utf8)+Data(repeating:255,count:8)
  let malformed:[(String,Data)] = [("static",prefix),("square-only",prefix+square),("bad-schema",badSchema),("bad-length",badLength),("wrong-uuid",wrongUUID),("truncated",truncated),("bad-extended",badExtended)]
  for (name,data) in malformed {
    guard try read(save(name+".m4a",data)) == nil else {throw NSError(domain:"Malformed or absent motion accepted: \(name)",code:1)}
  }
  let before=try FileManager.default.contentsOfDirectory(atPath:root.path).sorted()
  guard let source=MotionArtworkSource(url:parent) else {throw NSError(domain:"Cannot open bounded MP4 source",code:1)}
  let videos=try await source.asset.loadTracks(withMediaType:.video),audio=try await source.asset.loadTracks(withMediaType:.audio)
  try require(videos.count==1 && audio.isEmpty,"Unexpected media streams")
  let generator=AVAssetImageGenerator(asset:source.asset);generator.appliesPreferredTrackTransform=true
  let frame=try await generator.image(at:.zero)
  try require(frame.image.width==96 && frame.image.height==128,"Wrong portrait frame dimensions")
  let duration=try await source.asset.load(.duration).seconds
  try require(duration.isFinite && duration>0,"Missing clip duration")
  generator.maximumSize=CGSize(width:512,height:512)
  generator.requestedTimeToleranceBefore = .zero;generator.requestedTimeToleranceAfter = .zero
  let sampleTimes=[0.0,duration/3,duration*2/3]
  var forwardHashes=[String](),reverseHashes=[String]()
  for time in sampleTimes+Array(sampleTimes.reversed()) {
   let sample=try await generator.image(at:CMTime(seconds:time,preferredTimescale:600))
   guard let pixels=sample.image.dataProvider?.data else{throw NSError(domain:"Cannot inspect reverse frame pixels",code:1)}
   let digest=SHA256.hash(data:pixels as Data).map{String(format:"%02x",$0)}.joined()
   if forwardHashes.count<sampleTimes.count{forwardHashes.append(digest)}else{reverseHashes.append(digest)}
  }
  try require(Set(forwardHashes).count>1,"Reverse fixture must contain moving frames")
  try require(reverseHashes==Array(forwardHashes.reversed()),"Reverse frame requests must return the original frames in reverse order")
  generator.cancelAllCGImageGeneration();source.close();source.close()
  try require(try FileManager.default.contentsOfDirectory(atPath:root.path).sorted()==before,"Created a decoded/extracted cover cache")
  print("{\"passed\":true,\"portraitSelectedOverDefaultSquare\":true,\"malformedCases\":7,\"embeddedBytesPreserved\":true,\"decodedPortraitFrame\":true,\"reverseFramesVerified\":true,\"reversePauseAndResume\":true,\"audioTracks\":0,\"clipCacheCreated\":false,\"physicalDeviceTested\":false}")
 }
}
