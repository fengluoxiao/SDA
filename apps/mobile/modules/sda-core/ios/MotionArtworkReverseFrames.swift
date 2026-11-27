import AVFoundation
import CoreImage
import CoreGraphics

final class MotionArtworkReverseFrames {
 struct Frame {let time:Double;let image:CGImage;let blurredImage:CGImage;let bottomImage:CGImage;let bottomTone:CGColor;let bytes:Int}
 private struct Configuration {
  let track:AVAssetTrack,transform:CGAffineTransform,frameInterval:Double,windowDuration:Double,lastIndex:Int,duration:Double
 }
 private static let registryLock=NSLock()
 private static var registry=[String:MotionArtworkReverseFrames]()
 static func acquire(_ uri:String)->MotionArtworkReverseFrames {
  registryLock.lock();defer{registryLock.unlock()}
  if let existing=registry[uri]{existing.users+=1;return existing}
  let frames=MotionArtworkReverseFrames(uri);registry[uri]=frames;return frames
 }
 func release(){
  Self.registryLock.lock();users-=1;let finished=users==0
  if finished,Self.registry[uri] === self{Self.registry.removeValue(forKey:uri)}
  Self.registryLock.unlock()
  if finished{close()}
 }
 static let windowByteLimit=Int(min(48*1024*1024,max(12*1024*1024,ProcessInfo.processInfo.physicalMemory/128)))
 private let uri:String,lock=NSLock(),queue=DispatchQueue(label:"app.sda.motion-artwork.reverse",qos:.userInitiated)
 private let context=CIContext(options:[.cacheIntermediates:false])
 private var users=1,closed=false,working=false,wantedIndex:Int?,wantsTail=false,revision=0
 private var configuration:Configuration?,source:MotionArtworkSource?,reader:AVAssetReader?
 private var windows=[Int:[Frame]](),failure:Error?,configurationTask:Task<Void,Never>?
 private var decodedWindows=0,peakWindowBytes=0
 private var decodeSeconds=0.0,sharpSeconds=0.0,blurSeconds=0.0,bottomSeconds=0.0,toneSeconds=0.0
 private var activeViews=Set<ObjectIdentifier>()
 // The two visible surfaces share working RAM; an idle surface must not
 // discard frames still in use by the other surface.
 func setActive(_ active:Bool,for view:ObjectIdentifier){
  lock.lock()
  if active{activeViews.insert(view);lock.unlock();return}
  let removed=activeViews.remove(view) != nil
  guard removed,activeViews.isEmpty else{lock.unlock();return}
  let retired=resetLocked();lock.unlock();withExtendedLifetime(retired){}
 }
 private init(_ uri:String){
  self.uri=uri
  configurationTask=Task.detached(priority:.userInitiated){[weak self] in
   guard let self else{return}
   do{
    guard let url=URL(string:uri),let source=MotionArtworkSource(url:url) else{throw NSError(domain:"SDA.MotionReverse.Source",code:1)}
    do{
     guard let track=try await source.asset.loadTracks(withMediaType:.video).first else{throw NSError(domain:"SDA.MotionReverse.Track",code:1)}
     let duration=try await source.asset.load(.duration).seconds
     let naturalSize=try await track.load(.naturalSize),transform=try await track.load(.preferredTransform)
     let nominalRate=Double(try await track.load(.nominalFrameRate))
     let bounds=CGRect(origin:.zero,size:naturalSize).applying(transform)
     guard duration.isFinite,duration>0,duration<Double(Int.max)/120,bounds.width.isFinite,bounds.height.isFinite,bounds.width>0,bounds.height>0 else{throw NSError(domain:"SDA.MotionReverse.Dimensions",code:1)}
     let frameRate=nominalRate.isFinite && nominalRate>0 ? max(1,min(120,nominalRate)):30
     let sharpScale=min(1,1024/max(bounds.width,bounds.height)),blurScale=min(1,320/max(bounds.width,bounds.height))
     let sharpRow=((Int(ceil(bounds.width*sharpScale))*4+255)/256)*256
     let blurRow=((Int(ceil(bounds.width*blurScale))*4+255)/256)*256
     let bytes=sharpRow*Int(ceil(bounds.height*sharpScale))+blurRow*Int(ceil(bounds.height*blurScale))+blurRow*Int(ceil(bounds.height*blurScale*0.28))
     let frameCount=max(1,min(24,Self.windowByteLimit/max(1,bytes)))
     let windowDuration=Double(frameCount)/frameRate
     let configuration=Configuration(track:track,transform:transform,frameInterval:1/frameRate,windowDuration:windowDuration,lastIndex:max(0,Int(max(0,duration-0.5/frameRate)/windowDuration)),duration:duration)
     self.install(source,configuration)
    }catch{source.close();throw error}
   }catch{self.recordFailure(error)}
  }
 }
 private func install(_ source:MotionArtworkSource,_ configuration:Configuration){
  lock.lock()
  if closed{lock.unlock();source.close();return}
  self.source=source;self.configuration=configuration
  if wantsTail{wantedIndex=configuration.lastIndex;scheduleLocked()}
  lock.unlock()
 }
 private func recordFailure(_ error:Error){lock.lock();if !closed{failure=error};lock.unlock()}
 var failed:Bool{lock.lock();defer{lock.unlock()};return failure != nil}
 var frameInterval:Double{lock.lock();defer{lock.unlock()};return configuration?.frameInterval ?? 1.0/30}
 var statistics:(decodedWindows:Int,retainedBytes:Int,peakWindowBytes:Int){
  lock.lock();defer{lock.unlock()}
  return(decodedWindows,windows.values.flatMap{$0}.reduce(0){$0+$1.bytes},peakWindowBytes)
 }
 var renderingStatistics:[String:Double]{
  lock.lock();defer{lock.unlock()}
  return ["readerSeconds":decodeSeconds,"sharpSeconds":sharpSeconds,"blurSeconds":blurSeconds,"bottomSeconds":bottomSeconds,"toneSeconds":toneSeconds]
 }
 func prepareTail(){
  lock.lock();let retained=windows;defer{lock.unlock();withExtendedLifetime(retained){}};guard !closed else{return}
  wantsTail=true
  if let configuration{wantedIndex=configuration.lastIndex;pruneLocked();scheduleLocked()}
 }
 func frame(at position:Double)->Frame?{
  lock.lock();let retained=windows;defer{lock.unlock();withExtendedLifetime(retained){}}
  guard !closed,let configuration,position.isFinite else{return nil}
  let bounded=max(0,min(position,configuration.duration))
  let index=min(configuration.lastIndex,Int(bounded/configuration.windowDuration))
  if wantedIndex==nil || index<wantedIndex! || index>wantedIndex!+1 || (index>wantedIndex! && windows[index]==nil){wantedIndex=index;pruneLocked()}
  scheduleLocked()
  let frames=windows[index] ?? []
  return frames.min{abs($0.time-position)<abs($1.time-position)}
 }
 private func pruneLocked(){
  guard let wantedIndex else{windows.removeAll();return}
  windows=windows.filter{abs($0.key-wantedIndex)<=1}
 }
 private func scheduleLocked(){
  guard !closed,!working,failure==nil,configuration != nil,let wantedIndex else{return}
  let candidates=[wantedIndex,wantedIndex-1].filter{$0>=0 && windows[$0]==nil}
  guard !candidates.isEmpty else{return}
  working=true
  queue.async{[weak self] in self?.decodeWindows()}
 }
 private func decodeWindows(){
  while true{
   lock.lock()
   guard !closed,failure==nil,let configuration,let source,let wantedIndex,
    let index=[wantedIndex,wantedIndex-1].first(where:{$0>=0 && windows[$0]==nil}) else{working=false;lock.unlock();return}
   let token=revision;lock.unlock()
   do{
    let frames=try decode(index,configuration,source.asset,token)
    lock.lock();let retained=windows
    if !closed,revision==token,let currentIndex=self.wantedIndex,abs(index-currentIndex)<=1{
     windows[index]=frames;decodedWindows+=1;peakWindowBytes=max(peakWindowBytes,frames.reduce(0){$0+$1.bytes});pruneLocked()
    }
    lock.unlock()
    withExtendedLifetime(retained){}
   }catch{
    lock.lock();if !closed,revision==token{failure=error};lock.unlock()
   }
  }
 }
 private func decode(_ index:Int,_ configuration:Configuration,_ asset:AVAsset,_ token:Int)throws->[Frame]{
  let start=Double(index)*configuration.windowDuration,end=min(configuration.duration,start+configuration.windowDuration)
  let reader=try AVAssetReader(asset:asset)
  let output=AVAssetReaderTrackOutput(track:configuration.track,outputSettings:[kCVPixelBufferPixelFormatTypeKey as String:kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange])
  output.alwaysCopiesSampleData=false
  guard reader.canAdd(output) else{throw NSError(domain:"SDA.MotionReverse.Output",code:1)}
  reader.add(output);reader.timeRange=CMTimeRange(start:CMTime(seconds:start,preferredTimescale:60000),duration:CMTime(seconds:max(0,end-start),preferredTimescale:60000))
  lock.lock();guard !closed,revision==token else{lock.unlock();return []};self.reader=reader;lock.unlock()
  defer{reader.cancelReading();lock.lock();if self.reader === reader{self.reader=nil};lock.unlock()}
  guard reader.startReading() else{throw reader.error ?? NSError(domain:"SDA.MotionReverse.Start",code:1)}
  var frames=[Frame](),bytes=0
  var decoded=0.0,sharpRendered=0.0,blurRendered=0.0,bottomRendered=0.0,toneRendered=0.0
  defer{lock.lock();decodeSeconds+=decoded;sharpSeconds+=sharpRendered;blurSeconds+=blurRendered;bottomSeconds+=bottomRendered;toneSeconds+=toneRendered;lock.unlock()}
  while true{
   lock.lock();let shouldStop=closed || revision != token;lock.unlock()
   if shouldStop{break}
   let reading=Date.timeIntervalSinceReferenceDate
   let next=output.copyNextSampleBuffer();decoded+=Date.timeIntervalSinceReferenceDate-reading
   guard let sample=next else{break}
   lock.lock();let cancelled=closed || revision != token;lock.unlock()
   if cancelled{break}
   let keepReading:Bool=autoreleasepool{
    let time=CMSampleBufferGetPresentationTimeStamp(sample).seconds
    guard time>=start-0.00001,time<end,let buffer=CMSampleBufferGetImageBuffer(sample) else{return true}
    var image=CIImage(cvPixelBuffer:buffer).transformed(by:configuration.transform)
    image=image.transformed(by:CGAffineTransform(translationX:-image.extent.minX,y:-image.extent.minY))
    let scale=min(1,1024/max(image.extent.width,image.extent.height))
    let sharp=image.transformed(by:CGAffineTransform(scaleX:scale,y:scale))
    let blurScale=min(1,320/max(image.extent.width,image.extent.height))
    let small=image.transformed(by:CGAffineTransform(scaleX:blurScale,y:blurScale))
    let blurred=MotionArtworkBackdrop.blurred(small,bottomExtension:false,radius:36*blurScale*min(1,image.extent.width/720))
    let sharpStart=Date.timeIntervalSinceReferenceDate
    guard let sharpFrame=context.createCGImage(sharp,from:sharp.extent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB(),deferred:false) else{return false}
    sharpRendered+=Date.timeIntervalSinceReferenceDate-sharpStart
    let blurStart=Date.timeIntervalSinceReferenceDate
    guard let blurFrame=context.createCGImage(blurred,from:small.extent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB(),deferred:false),
     let bottomCrop=blurFrame.cropping(to:CGRect(x:0,y:Double(blurFrame.height)*0.72,width:Double(blurFrame.width),height:Double(blurFrame.height)*0.28)) else{return false}
    blurRendered+=Date.timeIntervalSinceReferenceDate-blurStart
    let bottomStart=Date.timeIntervalSinceReferenceDate
    let styledBottom=MotionArtworkBackdrop.continuationStyle(CIImage(cgImage:bottomCrop))
    guard let bottomFrame=context.createCGImage(styledBottom,from:styledBottom.extent,format:.RGBA8,colorSpace:CGColorSpaceCreateDeviceRGB(),deferred:false) else{return false}
    bottomRendered+=Date.timeIntervalSinceReferenceDate-bottomStart
    let toneStart=Date.timeIntervalSinceReferenceDate
    let bottomTone=MotionArtworkBackdrop.toneColor(sharpFrame)
    toneRendered+=Date.timeIntervalSinceReferenceDate-toneStart
    let frameBytes=sharpFrame.bytesPerRow*sharpFrame.height+blurFrame.bytesPerRow*blurFrame.height+bottomFrame.bytesPerRow*bottomFrame.height
    guard bytes+frameBytes<=Self.windowByteLimit else{return false}
    frames.append(Frame(time:time,image:sharpFrame,blurredImage:blurFrame,bottomImage:bottomFrame,bottomTone:bottomTone,bytes:frameBytes));bytes+=frameBytes;return true
   }
   if !keepReading{break}
  }
  if reader.status == .failed{throw reader.error ?? NSError(domain:"SDA.MotionReverse.Decode",code:1)}
  return frames.sorted{$0.time<$1.time}
 }
 func reset(){
  lock.lock();let retired=resetLocked();lock.unlock();withExtendedLifetime(retired){}
 }
 private func resetLocked()->[Int:[Frame]]{
  // Do not cancel a reader on the UI thread while copyNextSampleBuffer is
  // executing on the decode queue. Revision checks stop at the next sample;
  // decode's defer performs cancelReading on that same queue.
  revision+=1;let retired=windows;windows=[:];wantedIndex=nil;wantsTail=false;return retired
 }
 private func close(){
  lock.lock();closed=true;let retired=resetLocked();let source=self.source;self.source=nil;lock.unlock()
  configurationTask?.cancel();queue.async{source?.close()};withExtendedLifetime(retired){}
 }
 deinit{configurationTask?.cancel();reader?.cancelReading();source?.close()}
}
