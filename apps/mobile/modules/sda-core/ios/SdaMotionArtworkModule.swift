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
  AsyncFunction("motionArtworkInfo") { (uri:String) async -> [String:Any] in
   guard let url=URL(string:uri),let source=MotionArtworkSource(url:url) else{return ["present":false]}
   defer{source.close()}
   do{
    guard let track=try await source.asset.loadTracks(withMediaType:.video).first else{return ["present":false]}
    let size=try await track.load(.naturalSize),transform=try await track.load(.preferredTransform)
    let rect=CGRect(origin:.zero,size:size).applying(transform)
    guard rect.width.isFinite,rect.height.isFinite,rect.width>0,rect.height>0 else{return ["present":true]}
    return ["present":true,"width":Double(rect.width),"height":Double(rect.height)]
   }catch{return ["present":false]}
  }
  View(SdaMotionArtworkView.self) {
   Events("onStatus")
   Prop("sourceUri") { (view:SdaMotionArtworkView, value:String) in view.sourceUri=value }
   Prop("active") { (view:SdaMotionArtworkView, value:Bool) in view.active=value }
   Prop("presented") { (view:SdaMotionArtworkView, value:Bool) in view.presented=value }
   Prop("blurred") { (view:SdaMotionArtworkView, value:Bool) in view.blurred=value }
   Prop("bottomExtension") { (view:SdaMotionArtworkView, value:Bool) in view.bottomExtension=value }
   Prop("featherMode") { (view:SdaMotionArtworkView, value:String) in view.featherMode=value }
   Prop("featherHeight") { (view:SdaMotionArtworkView, value:Double) in view.featherHeight=value }
  }
 }
}
private final class MotionArtworkWeakView {
 weak var value:SdaMotionArtworkView?
 init(_ value:SdaMotionArtworkView){self.value=value}
}
final class SdaMotionArtworkView: ExpoView {
 private static var views=[MotionArtworkWeakView]()
 private var leader:SdaMotionArtworkView?{
  Self.views.removeAll{$0.value==nil}
  return Self.views.compactMap(\.value).first{$0 !== self && !$0.blurred && $0.sourceUri==sourceUri && $0.player != nil}
 }
 let onStatus=EventDispatcher()
 var sourceUri="" {didSet{if sourceUri != oldValue {stop();startIfNeeded()}}}
 var active=false {didSet{if active {startIfNeeded()}else{pause()}}}
 var presented=false {didSet{
  guard presented != oldValue else{return}
  if presented{startIfNeeded()}else{rewind()}
 }}
 var blurred=false {didSet{updateGravity();if blurred != oldValue && (player != nil || starting) {stop();startIfNeeded()}}}
 var bottomExtension=false {didSet{
  guard bottomExtension != oldValue else{return}
  backdrop.bottomExtension=bottomExtension;updateGravity()
  if !active,blurred,let item=player?.currentItem,let asset=source?.asset{item.videoComposition=makeVideoComposition(asset)}
 }}
 private let backdrop=MotionArtworkBackdrop()
 var featherMode="none" {didSet{updateFeather()}}
 var featherHeight=0.0 {didSet{updateFeather()}}
 private let feather=MotionArtworkMask()
 private let videoLayer=AVPlayerLayer()
 private var source:MotionArtworkSource?, player:AVQueuePlayer?,looper:AVPlayerLooper?
 private var ready:NSKeyValueObservation?, failure:NSKeyValueObservation?, loopFailure:NSKeyValueObservation?, observers=[NSObjectProtocol]()
 private var followerTimer:DispatchSourceTimer?,followerSeekPending=false,seekRevision=0
 private var generation=0,starting=false
 private var reportedReady=false
 required init(appContext:AppContext? = nil) {
  super.init(appContext:appContext);clipsToBounds=true;isUserInteractionEnabled=false;layer.masksToBounds=true
  Self.views.removeAll{$0.value==nil};Self.views.append(MotionArtworkWeakView(self))
  videoLayer.videoGravity = .resizeAspectFill;layer.addSublayer(videoLayer)
  let center=NotificationCenter.default
  observers.append(center.addObserver(forName:UIApplication.willResignActiveNotification,object:nil,queue:.main){[weak self]_ in self?.pause()})
  observers.append(center.addObserver(forName:UIApplication.didBecomeActiveNotification,object:nil,queue:.main){[weak self]_ in self?.startIfNeeded()})
 }
 override func layoutSubviews(){
  super.layoutSubviews();CATransaction.begin();CATransaction.setDisableActions(true);videoLayer.frame=bounds;CATransaction.commit()
  updateFeather()
  if canAnimate,bounds.width>0,bounds.height>0{startIfNeeded()}
 }
 private func updateFeather(){
  feather.update(size:bounds.size,mode:featherMode,height:featherHeight)
  CATransaction.begin();CATransaction.setDisableActions(true)
  let target=blurred || featherMode=="none" ? nil:feather.layer
  if layer.mask !== target{layer.mask=target}
  CATransaction.commit()
 }
 private func updateGravity(){
  CATransaction.begin();CATransaction.setDisableActions(true)
  videoLayer.videoGravity=blurred && bottomExtension ? .resize:.resizeAspectFill
  CATransaction.commit();updateFeather()
 }
 private func makeVideoComposition(_ asset:AVAsset)->AVVideoComposition {
  let policy=backdrop
  return AVVideoComposition(asset:asset){request in
   request.finish(with:policy.render(request.sourceImage),context:nil)
  }
 }
 override func didMoveToWindow(){super.didMoveToWindow();if window==nil{pause()}else{startIfNeeded()}}
 private func startIfNeeded() {
  guard window != nil,UIApplication.shared.applicationState == .active,let url=URL(string:sourceUri),url.isFileURL else{return}
  if player != nil {resume();return}
  guard !starting else{return}
  starting=true;let token=generation,uri=sourceUri
  Task.detached(priority:.userInitiated){[weak self] in
   var prepared=MotionArtworkSource(url:url)
   if let candidate=prepared {
    let duration=try? await candidate.asset.load(.duration)
    if !(duration?.seconds.isFinite ?? false) || (duration?.seconds ?? 0)<=0 {candidate.close();prepared=nil}
   }
   let loaded=prepared
   await MainActor.run{[weak self] in
    guard let self,self.generation==token else{loaded?.close();return}
    self.starting=false
    guard self.window != nil,UIApplication.shared.applicationState == .active else{loaded?.close();return}
    guard let loaded else{self.onStatus(["uri":uri,"ready":false,"unavailable":true]);return}
    let item=AVPlayerItem(asset:loaded.asset),player=AVQueuePlayer()
    if self.blurred{item.videoComposition=self.makeVideoComposition(loaded.asset)}
    self.source=loaded;self.player=player;player.isMuted=true;player.volume=0
    player.automaticallyWaitsToMinimizeStalling=false
    player.allowsExternalPlayback=false;player.preventsDisplaySleepDuringVideoPlayback=false
    // The native queue repeats the complete source, always in the forward direction.
    self.looper=AVPlayerLooper(player:player,templateItem:item);self.videoLayer.player=player
    self.ready=self.videoLayer.observe(\.isReadyForDisplay,options:[.initial,.new]){[weak self] layer,_ in
     DispatchQueue.main.async{[weak self] in
      guard let self,self.generation==token else{return}
      if layer.isReadyForDisplay{if !self.active || !self.presented {self.pause()};self.reportReady()}
     }
    }
    self.failure=player.observe(\.status,options:[.new]){[weak self] player,_ in
     if player.status == .failed{DispatchQueue.main.async{[weak self] in
      guard let self,self.generation==token else{return};self.stop();self.onStatus(["uri":uri,"ready":false,"unavailable":true])
     }}
    }
    self.loopFailure=self.looper?.observe(\.status,options:[.initial,.new]){[weak self] loop,_ in
     if loop.status == .failed{DispatchQueue.main.async{[weak self] in
      guard let self,self.generation==token else{return};self.stop();self.onStatus(["uri":uri,"ready":false,"unavailable":true])
     }}
    }
    // Prepare one real frame even when the current song is paused.
    player.playImmediately(atRate:1)
    if self.blurred,self.canAnimate{self.resume()}
   }
  }
 }
 private func stop(){
  generation+=1;starting=false;reportedReady=false;ready?.invalidate();ready=nil;failure?.invalidate();failure=nil;loopFailure?.invalidate();loopFailure=nil
  pause();looper?.disableLooping();looper=nil;player?.removeAllItems();videoLayer.player=nil;player=nil
  seekRevision+=1;followerSeekPending=false;source?.close();source=nil
 }
 private var canAnimate:Bool{active && presented && window != nil && UIApplication.shared.applicationState == .active}
 private func resume(){
  guard canAnimate,let player else{return}
  if blurred{
   if followerTimer==nil{
    let timer=DispatchSource.makeTimerSource(queue:.main)
    timer.schedule(deadline:.now(),repeating:.nanoseconds(16666667),leeway:.milliseconds(1))
    timer.setEventHandler{[weak self] in _=self?.followLeader()};followerTimer=timer;timer.resume()
   }
   if followLeader(){return}
  }
  player.playImmediately(atRate:1)
 }
 private func pause(){player?.pause();followerTimer?.cancel();followerTimer=nil}
 private func reportReady(){guard !reportedReady else{return};reportedReady=true;onStatus(["uri":sourceUri,"ready":true])}
 private func rewind(){pause();seekToStart()}
 // The blurred surface follows the sharp cover through loops and recreation.
 @discardableResult private func followLeader()->Bool{
  guard blurred,canAnimate,let leader,let player,let reference=leader.player else{return false}
  guard leader.canAnimate else{if player.rate != 0{player.pause()};return true}
  guard !followerSeekPending else{return true}
  let position=reference.currentTime().seconds,local=player.currentTime().seconds
  guard position.isFinite,local.isFinite else{return true}
  if abs(local-position)>0.12{
   player.pause();followerSeekPending=true;seekRevision+=1
   let revision=seekRevision,token=generation
   player.seek(to:CMTime(seconds:position,preferredTimescale:60000),toleranceBefore:CMTime(value:1,timescale:60),toleranceAfter:CMTime(value:1,timescale:60)){[weak self] finished in
    DispatchQueue.main.async{[weak self] in
     guard let self,self.generation==token,self.seekRevision==revision else{return}
     self.followerSeekPending=false
     guard finished,self.canAnimate,self.leader?.player === reference,self.leader?.canAnimate == true else{return}
     self.player?.playImmediately(atRate:1)
    }
   }
  }else if player.rate==0{player.playImmediately(atRate:1)}
  return true
 }
 private func seekToStart(){
  guard let player else{return}
  followerSeekPending=false;seekRevision+=1;let revision=seekRevision,token=generation
  player.seek(to:.zero,toleranceBefore:.zero,toleranceAfter:.zero){[weak self] finished in
   DispatchQueue.main.async{[weak self] in
    guard let self,finished,self.generation==token,self.seekRevision==revision else{return};self.resume()
   }
  }
 }
 deinit{
  for observer in observers{NotificationCenter.default.removeObserver(observer)}
  followerTimer?.cancel();player?.pause();looper?.disableLooping();player?.removeAllItems();source?.close()
 }
}
