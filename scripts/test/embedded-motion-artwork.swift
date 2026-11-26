import Foundation
import AVFoundation
import CryptoKit
import CoreImage

@main struct MotionArtworkTest {
 static func require(_ test:Bool,_ message:String) throws {if !test{throw NSError(domain:message,code:1)}}
 static func stressClip(_ url:URL)async throws {
  let writer=try AVAssetWriter(outputURL:url,fileType:.mp4)
  let input=AVAssetWriterInput(mediaType:.video,outputSettings:[AVVideoCodecKey:AVVideoCodecType.h264,AVVideoWidthKey:720,AVVideoHeightKey:1280,AVVideoCompressionPropertiesKey:[AVVideoAverageBitRateKey:2_000_000,AVVideoMaxKeyFrameIntervalKey:150,AVVideoExpectedSourceFrameRateKey:30,AVVideoAllowFrameReorderingKey:true]])
  input.expectsMediaDataInRealTime=false
  let adaptor=AVAssetWriterInputPixelBufferAdaptor(assetWriterInput:input,sourcePixelBufferAttributes:[kCVPixelBufferPixelFormatTypeKey as String:kCVPixelFormatType_32BGRA,kCVPixelBufferWidthKey as String:720,kCVPixelBufferHeightKey as String:1280,kCVPixelBufferIOSurfacePropertiesKey as String:[:]])
  try require(writer.canAdd(input),"Cannot create reverse stress fixture");writer.add(input)
  try require(writer.startWriting(),"Cannot start stress encoder");writer.startSession(atSourceTime:.zero)
  for index in 0..<150 {
   while !input.isReadyForMoreMediaData {
    if writer.status == .failed{throw writer.error ?? NSError(domain:"Stress encoder failed",code:1)}
    try await Task.sleep(nanoseconds:1_000_000)
   }
   guard let pool=adaptor.pixelBufferPool else{throw NSError(domain:"Stress pixel pool missing",code:1)}
   var allocated:CVPixelBuffer?
   try require(CVPixelBufferPoolCreatePixelBuffer(nil,pool,&allocated)==kCVReturnSuccess,"Stress pixel allocation failed")
   guard let buffer=allocated else{throw NSError(domain:"Stress buffer missing",code:1)}
   CVPixelBufferLockBaseAddress(buffer,[])
   let stride=CVPixelBufferGetBytesPerRow(buffer)/4
   let pixels=CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to:UInt32.self)
   pixels.initialize(repeating:0xff000000 | (UInt32(index)<<16) | 0x00003080,count:stride*1280)
   for row in 200..<280{pixels.advanced(by:row*stride+(index*4)%640).initialize(repeating:0xfff0e010,count:64)}
   CVPixelBufferUnlockBaseAddress(buffer,[])
   try require(adaptor.append(buffer,withPresentationTime:CMTime(value:Int64(index),timescale:30)),"Stress frame append failed")
  }
  writer.endSession(atSourceTime:CMTime(value:5,timescale:1));input.markAsFinished();await writer.finishWriting()
  try require(writer.status == .completed,"Stress encoder did not complete")
 }
 static func main() async throws {
  let colorExtent=CGRect(x:0,y:0,width:100,height:100)
  let green=CIImage(color:CIColor(red:0,green:1,blue:0)).cropped(to:colorExtent)
  let red=CIImage(color:CIColor(red:1,green:0,blue:0)).cropped(to:MotionArtworkBackdrop.bottomBand(colorExtent))
  let twoTone=red.composited(over:green)
  let extended=MotionArtworkBackdrop.blurred(twoTone,bottomExtension:true,radius:8)
  let renderContext=CIContext(options:[.cacheIntermediates:false])
  guard let colorFrame=renderContext.createCGImage(extended,from:colorExtent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB()),let colorData=colorFrame.dataProvider?.data else{throw NSError(domain:"Bottom-extension render failed",code:1)}
  let colorBytes=colorData as Data
  for row in [50,75,99]{let at=row*colorFrame.bytesPerRow+50*4;try require(Int(colorBytes[at])>Int(colorBytes[at+1])+10,"Playback controls did not retain the lower image colors")}
  try require(abs(MotionArtworkBackdrop.extendedImage(twoTone).extent.height-100)<0.00001,"Bottom extension changed the render bounds")
  for channel in [1,2]{
   let color=CIColor(red:0,green:channel==1 ? 1:0,blue:channel==2 ? 1:0)
   let lower=CIImage(color:color).cropped(to:MotionArtworkBackdrop.bottomBand(colorExtent)).composited(over:CIImage(color:CIColor(red:1,green:0,blue:0)).cropped(to:colorExtent))
   guard let painted=renderContext.createCGImage(MotionArtworkBackdrop.blurred(lower,bottomExtension:true,radius:8),from:colorExtent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB()),let data=painted.dataProvider?.data else{throw NSError(domain:"Generic bottom color render failed",code:1)}
   let bytes=data as Data
   for row in [50,75,99]{let at=row*painted.bytesPerRow+50*4;try require(Int(bytes[at+channel])>Int(bytes[at])+10,"Bottom extension used a fixed song color")}
  }
  let white=CIImage(color:CIColor(red:1,green:1,blue:1)).cropped(to:colorExtent)
  guard let whiteFrame=renderContext.createCGImage(MotionArtworkBackdrop.blurred(white,bottomExtension:true,radius:8),from:colorExtent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB()),let whiteData=whiteFrame.dataProvider?.data else{throw NSError(domain:"White artwork render failed",code:1)}
  let whiteBytes=whiteData as Data
  try require(whiteBytes[50*whiteFrame.bytesPerRow+50*4]<=165,"Bright artwork must remain dark enough under playback controls")
  guard let rawColorFrame=renderContext.createCGImage(twoTone,from:colorExtent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB(),deferred:false) else{throw NSError(domain:"Palette reference render failed",code:1)}
  let forwardTone=MotionArtworkBackdrop.toneColor(twoTone),reverseTone=MotionArtworkBackdrop.toneColor(rawColorFrame)
  guard let forwardComponents=forwardTone.components,let reverseComponents=reverseTone.components else{throw NSError(domain:"Palette components missing",code:1)}
  for channel in 0..<3{try require(abs(forwardComponents[channel]-reverseComponents[channel])<0.02,"Forward and reverse palette samplers disagree")}
  try require(abs(reverseTone.alpha-0.85)<0.00001,"Footer tone opacity changed")
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
  let stressURL=root.appendingPathComponent("long-gop.mp4")
  try await stressClip(stressURL)
  let stressParent=try save("long-gop.m4a",prefix+motion("tall",try Data(contentsOf:stressURL)))
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
  let buffered=MotionArtworkReverseFrames.acquire(parent.absoluteString)
  let backgroundClient=MotionArtworkReverseFrames.acquire(parent.absoluteString)
  try require(buffered === backgroundClient,"Sharp and blurred covers must share one decoder")
  backgroundClient.release();defer{buffered.release()};buffered.prepareTail()
  func bufferedFrame(_ position:Double)async throws->MotionArtworkReverseFrames.Frame {
   let deadline=Date().addingTimeInterval(20)
   while Date()<deadline {
    if let frame=buffered.frame(at:position){return frame}
    try require(!buffered.failed,"Windowed reverse decoder failed")
    try await Task.sleep(nanoseconds:5_000_000)
   }
   throw NSError(domain:"Windowed reverse frame timed out",code:1)
  }
  _=try await bufferedFrame(duration-1.0/30)
  var bufferedForward=[String](),bufferedReverse=[String]()
  for time in sampleTimes+Array(sampleTimes.reversed()) {
   let sample=try await bufferedFrame(time)
   try require(sample.bottomImage.width>0 && sample.bottomImage.height>0,"Reverse frame is missing its bottom continuation")
   guard let pixels=sample.image.dataProvider?.data else{throw NSError(domain:"Missing buffered pixels",code:1)}
   let hash=SHA256.hash(data:pixels as Data).map{String(format:"%02x",$0)}.joined()
   if bufferedForward.count<sampleTimes.count{bufferedForward.append(hash)}else{bufferedReverse.append(hash)}
  }
  try require(bufferedReverse==Array(bufferedForward.reversed()),"Windowed playback changed the reverse frame order")
  buffered.prepareTail();_=try await bufferedFrame(duration-buffered.frameInterval)
  let frameCount=max(1,Int(duration/buffered.frameInterval)),reverseStarted=Date()
  var delivered=0,lastTime=Double.infinity
  for index in stride(from:frameCount-1,through:0,by:-1) {
   let frame=try await bufferedFrame(Double(index)*buffered.frameInterval)
   try require(frame.time<=lastTime+0.00001,"Reverse samples must have descending timestamps")
   if frame.time != lastTime{delivered+=1};lastTime=frame.time
   try require(buffered.statistics.retainedBytes<=3*MotionArtworkReverseFrames.windowByteLimit,"Reverse windows exceeded the memory bound")
  }
  let reverseSeconds=Date().timeIntervalSince(reverseStarted),stats=buffered.statistics
  try require(delivered>=max(2,frameCount*4/5),"Windowed playback skipped source frames")
  try require(stats.peakWindowBytes<=MotionArtworkReverseFrames.windowByteLimit,"Decoded window exceeded its byte budget")
  try require(reverseSeconds<=max(2,duration*1.5),"Windowed decoder cannot keep up with the synthetic clip")
  print("{\"windowedReverseFrames\":\(delivered),\"decodeSeconds\":\(reverseSeconds),\"decodeFramesPerSecond\":\(Double(delivered)/max(reverseSeconds,0.000001)),\"decodedWindows\":\(stats.decodedWindows),\"retainedBytes\":\(stats.retainedBytes),\"peakWindowBytes\":\(stats.peakWindowBytes)}")
  let sharpView=NSObject(),blurredView=NSObject()
  buffered.setActive(true,for:ObjectIdentifier(sharpView));buffered.setActive(true,for:ObjectIdentifier(blurredView))
  buffered.setActive(false,for:ObjectIdentifier(sharpView))
  try require(buffered.statistics.retainedBytes>0,"Pausing one surface discarded the other active surface's working frames")
  let frozen=try await bufferedFrame(duration/2)
  buffered.setActive(false,for:ObjectIdentifier(blurredView))
  try require(buffered.statistics.retainedBytes==0,"Pausing both surfaces must discard reverse working frames")
  try await Task.sleep(nanoseconds:100_000_000)
  try require(buffered.statistics.retainedBytes==0,"Cancelled decode republished frames after pause")
  try require(frozen.image.width>0 && frozen.blurredImage.width>0,"Dropping working windows invalidated the displayed frame")
  buffered.setActive(true,for:ObjectIdentifier(sharpView))
  let resumed=try await bufferedFrame(duration/2)
  try require(resumed.time==frozen.time,"Resume after dropping windows changed the selected frame")
  buffered.setActive(false,for:ObjectIdentifier(sharpView))
  buffered.reset();try require(buffered.statistics.retainedBytes==0,"Track reset must discard reverse windows")
  let stress=MotionArtworkReverseFrames.acquire(stressParent.absoluteString);defer{stress.release()};stress.prepareTail()
  func stressFrame(_ position:Double)async throws->MotionArtworkReverseFrames.Frame {
   let deadline=Date().addingTimeInterval(20)
   while Date()<deadline {
    if let frame=stress.frame(at:position){return frame}
    try require(!stress.failed,"Long-GOP reverse decoder failed");try await Task.sleep(nanoseconds:2_000_000)
   }
   throw NSError(domain:"Long-GOP reverse frame timed out",code:1)
  }
  _=try await stressFrame(149.0/30)
  let stressStarted=Date();var stressDelivered=0,previousTime=Double.infinity
  for index in stride(from:149,through:0,by:-1){
   let frame=try await stressFrame(Double(index)/30)
   try require(frame.time<=previousTime+0.00001,"Long-GOP timestamps must descend")
   if frame.time != previousTime{stressDelivered+=1};previousTime=frame.time
   try require(stress.statistics.retainedBytes<=3*MotionArtworkReverseFrames.windowByteLimit,"Long-GOP working memory exceeded its bound")
  }
  let stressSeconds=Date().timeIntervalSince(stressStarted)
  try require(stressDelivered>=140,"Long-GOP reverse playback dropped source frames")
  print("{\"longGopResolution\":\"720x1280\",\"reverseFrames\":\(stressDelivered),\"decodeSeconds\":\(stressSeconds),\"decodeFramesPerSecond\":\(Double(stressDelivered)/max(stressSeconds,0.000001)),\"retainedBytes\":\(stress.statistics.retainedBytes),\"windowByteLimit\":\(MotionArtworkReverseFrames.windowByteLimit)}")
  try require(Double(stressDelivered)/max(stressSeconds,0.000001)>=24,"Long-GOP reverse decode remains below 24 frames per second")
  stress.reset();try require(stress.statistics.retainedBytes==0,"Long-GOP windows survived reset")
  generator.cancelAllCGImageGeneration();source.close();source.close()
  try require(try FileManager.default.contentsOfDirectory(atPath:root.path).sorted()==before,"Created a decoded/extracted cover cache")
  print("{\"passed\":true,\"portraitSelectedOverDefaultSquare\":true,\"malformedCases\":7,\"embeddedBytesPreserved\":true,\"decodedPortraitFrame\":true,\"reverseFramesVerified\":true,\"reversePauseAndResume\":true,\"audioTracks\":0,\"clipCacheCreated\":false,\"physicalDeviceTested\":false}")
 }
}
