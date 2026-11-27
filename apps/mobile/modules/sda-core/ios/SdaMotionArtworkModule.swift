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
  if fallbackReverse,let frame=blurred && leader != nil ? leader?.latestReverseFrame:reverseFrames?.frame(at:reverseClock.current(at:CACurrentMediaTime())){lastReverseFrameTime=nil;showReverseFrame(frame)}
  else if !active,blurred,let item=player?.currentItem,let asset=source?.asset{item.videoComposition=makeVideoComposition(asset)}
 }}
 private let backdrop=MotionArtworkBackdrop()
 var featherMode="none" {didSet{updateFeather()}}
 var featherHeight=0.0 {didSet{updateFeather()}}
 private let feather=MotionArtworkMask()
 private let videoLayer=AVPlayerLayer()
 private var source:MotionArtworkSource?, player:AVPlayer?
 private var ready:NSKeyValueObservation?, failure:NSKeyValueObservation?, observers=[NSObjectProtocol]()
 private var endObserver:NSObjectProtocol?,timeObserver:Any?
 private var direction:Float=1,fallbackReverse=false
 private var awaitingReverseFrame=false
 private var reverseClock=MotionArtworkReverseClock()
 private var reverseFrames:MotionArtworkReverseFrames?,reverseTimer:DispatchSourceTimer?
 private var followerTimer:DispatchSourceTimer?,followerSeekPending=false
 private var latestReverseFrame:MotionArtworkReverseFrames.Frame?
 private var lastReverseFrameTime:Double?,seekRevision=0
 private let reverseLayer=CALayer()
 private let toneLayer=CAGradientLayer()
 private var generation=0,starting=false
 required init(appContext:AppContext? = nil) {
  super.init(appContext:appContext);clipsToBounds=true;isUserInteractionEnabled=false;layer.masksToBounds=true
  Self.views.removeAll{$0.value==nil};Self.views.append(MotionArtworkWeakView(self))
  videoLayer.videoGravity = .resizeAspectFill;layer.addSublayer(videoLayer)
  reverseLayer.contentsGravity = .resizeAspectFill;reverseLayer.masksToBounds=true;reverseLayer.isHidden=true;layer.addSublayer(reverseLayer)
  toneLayer.startPoint=CGPoint(x:0.5,y:0);toneLayer.endPoint=CGPoint(x:0.5,y:1);toneLayer.locations=[0,0.6,1];toneLayer.isHidden=true;layer.addSublayer(toneLayer)
  let center=NotificationCenter.default
  observers.append(center.addObserver(forName:UIApplication.willResignActiveNotification,object:nil,queue:.main){[weak self]_ in self?.pause()})
  observers.append(center.addObserver(forName:UIApplication.didBecomeActiveNotification,object:nil,queue:.main){[weak self]_ in self?.startIfNeeded()})
 }
 override func layoutSubviews(){
  super.layoutSubviews();CATransaction.begin();CATransaction.setDisableActions(true);videoLayer.frame=bounds;reverseLayer.frame=bounds;toneLayer.frame=bounds;CATransaction.commit()
  updateFeather()
  // Mask/layout changes can detach and pause an existing player without
  // changing the React active prop. Reconcile after its visible bounds return.
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
  reverseLayer.contentsGravity=blurred && bottomExtension ? .resize:.resizeAspectFill
  toneLayer.isHidden = !(blurred && bottomExtension)
  CATransaction.commit()
  updateFeather()
 }
 private func makeVideoComposition(_ asset:AVAsset)->AVVideoComposition {
  let policy=backdrop,token=generation
  return AVVideoComposition(asset:asset){[weak self] request in
   let output=policy.render(request.sourceImage)
   if policy.bottomExtension {
    let tone=MotionArtworkBackdrop.toneColor(request.sourceImage)
    DispatchQueue.main.async{[weak self] in guard let self,self.generation==token,!self.fallbackReverse,self.bottomExtension else{return};self.paintTone(tone)}
   }
   request.finish(with:output,context:nil)
  }
 }
 private func paintTone(_ color:CGColor){
  CATransaction.begin();CATransaction.setDisableActions(true);toneLayer.colors=[color.copy(alpha:0)!,color.copy(alpha:0)!,color];CATransaction.commit()
 }
 override func didMoveToWindow(){super.didMoveToWindow();if window==nil{pause()}else{startIfNeeded()}}
 private func startIfNeeded() {
  guard window != nil,UIApplication.shared.applicationState == .active,let url=URL(string:sourceUri),url.isFileURL else{return}
  if player != nil {resume();return}
   guard !starting else{return}
  starting=true;let token=generation,uri=sourceUri
  DispatchQueue.global(qos:.userInitiated).async { [weak self] in
   let prepared=MotionArtworkSource(url:url)
   DispatchQueue.main.async { [weak self] in
    guard let self,self.generation==token else{prepared?.close();return}
    self.starting=false
    guard self.window != nil,UIApplication.shared.applicationState == .active else{prepared?.close();return}
    guard let prepared else {self.onStatus(["uri":uri,"ready":false,"unavailable":true]);return}
    let item=AVPlayerItem(asset:prepared.asset),player=AVPlayer()
    if self.blurred {item.videoComposition=self.makeVideoComposition(prepared.asset)}
    self.source=prepared;self.player=player;self.reverseFrames=MotionArtworkReverseFrames.acquire(uri);player.isMuted=true;player.volume=0
    self.reverseFrames?.setActive(self.canAnimate,for:ObjectIdentifier(self))
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
    if self.blurred,self.canAnimate{self.resume()}
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
  followerSeekPending=false;latestReverseFrame=nil
  CATransaction.begin();CATransaction.setDisableActions(true);reverseLayer.contents=nil;reverseLayer.isHidden=true;toneLayer.colors=nil;CATransaction.commit()
  source?.close();source=nil
 }
 private var canAnimate:Bool{active && presented && window != nil && UIApplication.shared.applicationState == .active}
 private func resume() {
  guard canAnimate,let player else{return}
  reverseFrames?.setActive(true,for:ObjectIdentifier(self))
  if blurred{
   if followerTimer==nil{
    let timer=DispatchSource.makeTimerSource(queue:.main)
    timer.schedule(deadline:.now(),repeating:.nanoseconds(16666667),leeway:.milliseconds(1))
    timer.setEventHandler{[weak self] in _=self?.followLeader()};followerTimer=timer;timer.resume()
   }
   if followLeader(){return}
  }
  if fallbackReverse {
   guard reverseTimer==nil else{return}
   let timer=DispatchSource.makeTimerSource(queue:.main)
   timer.schedule(deadline:.now(),repeating:.nanoseconds(16666667),leeway:.milliseconds(1))
   timer.setEventHandler{[weak self] in self?.renderReverseFrame()};reverseTimer=timer;timer.resume()
  }else{prepareReverseTail();player.playImmediately(atRate:direction)}
 }
 private func pause() {
  player?.pause();reverseClock.pause(at:CACurrentMediaTime())
  reverseTimer?.cancel();reverseTimer=nil
  followerTimer?.cancel();followerTimer=nil
  awaitingReverseFrame=fallbackReverse
  reverseFrames?.setActive(false,for:ObjectIdentifier(self))
 }
 private func prepareReverseTail(){
  if blurred,leader != nil{return}
  guard let player,let duration=player.currentItem?.duration.seconds,duration.isFinite,player.currentTime().seconds>=duration-1.5 else{return}
  reverseFrames?.prepareTail()
 }
 private func beginReverse() {
  if blurred,followLeader(){return}
  if !presented{rewind();return}
  guard let player,let item=player.currentItem else{return}
  let duration=item.duration.seconds
  guard duration.isFinite,duration>0 else{return}
  direction = -1
  fallbackReverse=true;player.pause()
  awaitingReverseFrame=true
  reverseFrames?.prepareTail();lastReverseFrameTime=nil;latestReverseFrame=nil
  reverseClock.begin(at:max(0,duration-(reverseFrames?.frameInterval ?? 1.0/30)));resume()
 }
 private func renderReverseFrame() {
  guard canAnimate,fallbackReverse,let frames=reverseFrames else{return}
  if blurred,followLeader(){return}
  let position=reverseClock.current(at:CACurrentMediaTime())
  if position<=0{playForwardFromStart();return}
  guard let frame=frames.frame(at:position) else{
   reverseClock.pause(at:CACurrentMediaTime());awaitingReverseFrame=true
   if frames.failed{let uri=sourceUri;stop();onStatus(["uri":uri,"ready":false,"unavailable":true])};return
  }
  if awaitingReverseFrame{awaitingReverseFrame=false;reverseClock.resume(at:CACurrentMediaTime())}
  showReverseFrame(frame)
 }
 private func showReverseFrame(_ frame:MotionArtworkReverseFrames.Frame){
  latestReverseFrame=frame
  guard lastReverseFrameTime != frame.time else{return}
  lastReverseFrameTime=frame.time
  if blurred && bottomExtension{paintTone(frame.bottomTone)}
  CATransaction.begin();CATransaction.setDisableActions(true);reverseLayer.contents=blurred ? (bottomExtension ? frame.bottomImage:frame.blurredImage):frame.image;reverseLayer.isHidden=false;CATransaction.commit()
 }
 private func playForwardFromStart() {
  pause();direction=1;fallbackReverse=false;lastReverseFrameTime=nil
  seekToStart()
 }
 private func rewind() {
  pause();direction=1;fallbackReverse=false;lastReverseFrameTime=nil;latestReverseFrame=nil;reverseFrames?.reset();reverseClock.begin(at:0)
  seekToStart()
 }
 // Only the sharp surface owns the playback clock and requests reverse
 // windows. Recreating the blurred surface must never start a competing clock.
 @discardableResult private func followLeader()->Bool{
  guard blurred,canAnimate,let leader,let player,let reference=leader.player else{return false}
  reverseTimer?.cancel();reverseTimer=nil
  guard leader.canAnimate else{player.pause();return true}
  if leader.fallbackReverse{
   if !fallbackReverse{lastReverseFrameTime=nil;seekRevision+=1;followerSeekPending=false}
   fallbackReverse=true;direction = -1;player.pause()
   if let frame=leader.latestReverseFrame{showReverseFrame(frame)}
   return true
  }
  let changed=fallbackReverse;fallbackReverse=false;direction=1
  guard !followerSeekPending else{return true}
  let position=reference.currentTime().seconds,local=player.currentTime().seconds
  guard position.isFinite,local.isFinite else{return true}
  if changed || abs(local-position)>max(0.12,(reverseFrames?.frameInterval ?? 1.0/30)*4){
   player.pause();followerSeekPending=true;seekRevision+=1
   let revision=seekRevision,token=generation
   player.seek(to:CMTime(seconds:position,preferredTimescale:60000),toleranceBefore:CMTime(value:1,timescale:60),toleranceAfter:CMTime(value:1,timescale:60)){[weak self] finished in
    DispatchQueue.main.async{[weak self] in
     guard let self,self.generation==token,self.seekRevision==revision else{return}
     self.followerSeekPending=false
     guard finished,self.canAnimate,self.leader?.fallbackReverse==false else{return}
     CATransaction.begin();CATransaction.setDisableActions(true);self.reverseLayer.isHidden=true;self.reverseLayer.contents=nil;CATransaction.commit()
     self.player?.playImmediately(atRate:1)
    }
   }
  }else{
   if player.rate==0{player.playImmediately(atRate:1)}
  }
  return true
 }
 private func seekToStart() {
  guard let player else{return}
  seekRevision+=1;let revision=seekRevision,token=generation
  player.seek(to:.zero,toleranceBefore:.zero,toleranceAfter:.zero){[weak self] finished in
   DispatchQueue.main.async{[weak self] in
    guard let self,finished,self.generation==token,self.seekRevision==revision else{return}
    self.latestReverseFrame=nil
    CATransaction.begin();CATransaction.setDisableActions(true);self.reverseLayer.isHidden=true;self.reverseLayer.contents=nil;CATransaction.commit()
    self.resume()
   }
  }
 }
 deinit {
  for observer in observers{NotificationCenter.default.removeObserver(observer)}
  if let endObserver{NotificationCenter.default.removeObserver(endObserver)}
  if let timeObserver{player?.removeTimeObserver(timeObserver)}
  reverseTimer?.cancel();followerTimer?.cancel();reverseFrames?.setActive(false,for:ObjectIdentifier(self));reverseFrames?.release();player?.pause();source?.close()
 }
}
