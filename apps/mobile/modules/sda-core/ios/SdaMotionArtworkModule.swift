import ExpoModulesCore
import AVFoundation
import UIKit
import CoreImage
import QuartzCore

public final class SdaMotionArtworkModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaMotionArtwork")
  AsyncFunction("hasMotionArtwork") { (uri:String) -> Bool in
   guard let url=URL(string:uri),url.isFileURL else{return false}
   let scoped=url.startAccessingSecurityScopedResource()
   defer{if scoped{url.stopAccessingSecurityScopedResource()}}
   guard let file=try? FileHandle(forReadingFrom:url) else{return false}
   defer{try? file.close()}
   do{return try MotionArtworkRange.find(in:file,size:file.seekToEnd()) != nil}catch{return false}
  }
  View(SdaMotionArtworkView.self) {
   Events("onStatus")
   Prop("sourceUri") { (view:SdaMotionArtworkView, value:String) in view.sourceUri=value }
   Prop("active") { (view:SdaMotionArtworkView, value:Bool) in view.active=value }
   Prop("presented") { (view:SdaMotionArtworkView, value:Bool) in view.presented=value }
   Prop("blurred") { (view:SdaMotionArtworkView, value:Bool) in view.blurred=value }
  }
 }
}
final class SdaMotionArtworkView: ExpoView {
 let onStatus=EventDispatcher()
 var sourceUri="" {didSet{if sourceUri != oldValue {stop();startIfNeeded()}}}
 var active=false {didSet{if active {startIfNeeded()}else{pause()}}}
 var presented=false {didSet{
  guard presented != oldValue else{return}
  if presented{startIfNeeded()}else{rewind()}
 }}
 var blurred=false {didSet{if blurred != oldValue && (player != nil || starting) {stop();startIfNeeded()}}}
 private let videoLayer=AVPlayerLayer()
 private var source:MotionArtworkSource?, player:AVPlayer?
 private var ready:NSKeyValueObservation?, failure:NSKeyValueObservation?, observers=[NSObjectProtocol]()
 private var endObserver:NSObjectProtocol?,timeObserver:Any?
 private var direction:Float=1,fallbackReverse=false
 private var reverseClock=MotionArtworkReverseClock()
 private var reverseFrames:MotionArtworkReverseFrames?,reverseTimer:DispatchSourceTimer?
 private var lastReverseFrameTime:Double?,seekRevision=0
 private let reverseLayer=CALayer()
 private var generation=0,starting=false
 required init(appContext:AppContext? = nil) {
  super.init(appContext:appContext);clipsToBounds=true;isUserInteractionEnabled=false;layer.masksToBounds=true
  videoLayer.videoGravity = .resizeAspectFill;layer.addSublayer(videoLayer)
  reverseLayer.contentsGravity = .resizeAspectFill;reverseLayer.masksToBounds=true;reverseLayer.isHidden=true;layer.addSublayer(reverseLayer)
  let center=NotificationCenter.default
  observers.append(center.addObserver(forName:UIApplication.willResignActiveNotification,object:nil,queue:.main){[weak self]_ in self?.pause()})
  observers.append(center.addObserver(forName:UIApplication.didBecomeActiveNotification,object:nil,queue:.main){[weak self]_ in self?.startIfNeeded()})
 }
 override func layoutSubviews(){super.layoutSubviews();CATransaction.begin();CATransaction.setDisableActions(true);videoLayer.frame=bounds;reverseLayer.frame=bounds;CATransaction.commit()}
 override func didMoveToWindow(){super.didMoveToWindow();if window==nil{pause()}else{startIfNeeded()}}
 private func startIfNeeded() {
  guard window != nil,UIApplication.shared.applicationState == .active,let url=URL(string:sourceUri),url.isFileURL else{return}
  if player != nil {resume();return}
   guard !starting else{return}
  starting=true;let token=generation,uri=sourceUri
  DispatchQueue.global(qos:.userInitiated).async { [weak self] in
   let prepared=MotionArtworkSource(url:url)
   DispatchQueue.main.async { [weak self] in
    guard let self,self.generation==token,self.window != nil,UIApplication.shared.applicationState == .active else{prepared?.close();return}
    self.starting=false
    guard let prepared else {self.onStatus(["uri":uri,"ready":false,"unavailable":true]);return}
    let item=AVPlayerItem(asset:prepared.asset),player=AVPlayer()
    if self.blurred {item.videoComposition=AVVideoComposition(asset:prepared.asset){request in
     let source=request.sourceImage.clampedToExtent()
     let filter=CIFilter(name:"CIGaussianBlur",parameters:[kCIInputRadiusKey:36])!
     filter.setValue(source,forKey:kCIInputImageKey)
     guard let output=filter.outputImage else {request.finish(with:source,context:nil);return}
     request.finish(with:output.cropped(to:request.sourceImage.extent),context:nil)
    }}
    self.source=prepared;self.player=player;self.reverseFrames=MotionArtworkReverseFrames.acquire(uri);player.isMuted=true;player.volume=0
    player.automaticallyWaitsToMinimizeStalling=false
    player.allowsExternalPlayback=false;player.preventsDisplaySleepDuringVideoPlayback=false
    player.replaceCurrentItem(with:item);self.videoLayer.player=player
    self.endObserver=NotificationCenter.default.addObserver(forName:AVPlayerItem.didPlayToEndTimeNotification,object:item,queue:.main){[weak self]_ in
     guard let self,self.generation==token else{return}
     let position=self.player?.currentTime().seconds ?? 0
     if self.direction>0 {
      guard let duration=self.player?.currentItem?.duration.seconds,duration.isFinite,position>=duration-1.0/60 else{return}
      self.beginReverse()
     }else if position<=1.0/60{self.playForwardFromStart()}
    }
    self.timeObserver=player.addPeriodicTimeObserver(forInterval:CMTime(value:1,timescale:5),queue:.main){[weak self] _ in
     guard let self,self.generation==token,self.direction>0,self.canAnimate else{return}
     self.prepareReverseTail()
    }
    self.ready=self.videoLayer.observe(\.isReadyForDisplay,options:[.initial,.new]) { [weak self] layer,_ in
     DispatchQueue.main.async { [weak self] in guard let self,self.generation==token else{return}
      if layer.isReadyForDisplay {
       if !self.active || !self.presented {self.pause()}
       self.onStatus(["uri":uri,"ready":true])
      }
     }
    }
    self.failure=player.observe(\.status,options:[.new]) { [weak self] player,_ in
     if player.status == .failed {DispatchQueue.main.async { [weak self] in guard let self,self.generation==token else{return};self.stop();self.onStatus(["uri":uri,"ready":false,"unavailable":true])}}
    }
    player.playImmediately(atRate:1)
   }
  }
 }
 private func stop() {
  generation+=1;starting=false;ready?.invalidate();ready=nil;failure?.invalidate();failure=nil
  if let endObserver{NotificationCenter.default.removeObserver(endObserver)};endObserver=nil
  if let timeObserver{player?.removeTimeObserver(timeObserver)};timeObserver=nil
  pause();reverseFrames?.release();reverseFrames=nil
  player?.replaceCurrentItem(with:nil);videoLayer.player=nil;player=nil
  direction=1;fallbackReverse=false;reverseClock.begin(at:0);seekRevision+=1
  CATransaction.begin();CATransaction.setDisableActions(true);reverseLayer.contents=nil;reverseLayer.isHidden=true;CATransaction.commit()
  source?.close();source=nil
 }
 private var canAnimate:Bool{active && presented && window != nil && UIApplication.shared.applicationState == .active}
 private func resume() {
  guard canAnimate,let player else{return}
  if fallbackReverse {
   reverseClock.resume(at:CACurrentMediaTime())
   guard reverseTimer==nil else{return}
   let timer=DispatchSource.makeTimerSource(queue:.main)
   timer.schedule(deadline:.now(),repeating:.nanoseconds(16666667),leeway:.milliseconds(1))
   timer.setEventHandler{[weak self] in self?.renderReverseFrame()};reverseTimer=timer;timer.resume()
  }else{prepareReverseTail();player.playImmediately(atRate:direction)}
 }
 private func pause() {
  player?.pause();reverseClock.pause(at:CACurrentMediaTime())
  reverseTimer?.cancel();reverseTimer=nil
 }
 private func prepareReverseTail(){
  guard let player,let duration=player.currentItem?.duration.seconds,duration.isFinite,player.currentTime().seconds>=duration-1.5 else{return}
  reverseFrames?.prepareTail()
 }
 private func beginReverse() {
  if !presented{rewind();return}
  guard let player,let item=player.currentItem else{return}
  let duration=item.duration.seconds
  guard duration.isFinite,duration>0 else{return}
  direction = -1
  fallbackReverse=true;player.pause()
  reverseFrames?.prepareTail();lastReverseFrameTime=nil
  reverseClock.begin(at:max(0,duration-(reverseFrames?.frameInterval ?? 1.0/30)));resume()
 }
 private func renderReverseFrame() {
  guard canAnimate,fallbackReverse,let frames=reverseFrames else{return}
  let position=reverseClock.current(at:CACurrentMediaTime())
  if position<=0{playForwardFromStart();return}
  guard let frame=frames.frame(at:position) else{
   if frames.failed{let uri=sourceUri;stop();onStatus(["uri":uri,"ready":false,"unavailable":true])};return
  }
  guard lastReverseFrameTime != frame.time else{return}
  lastReverseFrameTime=frame.time
  CATransaction.begin();CATransaction.setDisableActions(true);reverseLayer.contents=blurred ? frame.blurredImage:frame.image;reverseLayer.isHidden=false;CATransaction.commit()
 }
 private func playForwardFromStart() {
  pause();direction=1;fallbackReverse=false;lastReverseFrameTime=nil
  seekToStart()
 }
 private func rewind() {
  pause();direction=1;fallbackReverse=false;lastReverseFrameTime=nil;reverseFrames?.reset();reverseClock.begin(at:0)
  seekToStart()
 }
 private func seekToStart() {
  guard let player else{return}
  seekRevision+=1;let revision=seekRevision,token=generation
  player.seek(to:.zero,toleranceBefore:.zero,toleranceAfter:.zero){[weak self] finished in
   DispatchQueue.main.async{[weak self] in
    guard let self,finished,self.generation==token,self.seekRevision==revision else{return}
    CATransaction.begin();CATransaction.setDisableActions(true);self.reverseLayer.isHidden=true;self.reverseLayer.contents=nil;CATransaction.commit()
    self.resume()
   }
  }
 }
 deinit {
  for observer in observers{NotificationCenter.default.removeObserver(observer)}
  if let endObserver{NotificationCenter.default.removeObserver(endObserver)}
  if let timeObserver{player?.removeTimeObserver(timeObserver)}
  reverseTimer?.cancel();reverseFrames?.release();player?.pause();source?.close()
 }
}
