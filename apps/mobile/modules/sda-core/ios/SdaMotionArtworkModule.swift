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
 private var source:MotionArtworkSource?, player:AVQueuePlayer?, loop:AVPlayerLooper?
 private var ready:NSKeyValueObservation?, failure:NSKeyValueObservation?, observers=[NSObjectProtocol]()
 private var generation=0,starting=false
 required init(appContext:AppContext? = nil) {
  super.init(appContext:appContext);clipsToBounds=true;isUserInteractionEnabled=false;layer.masksToBounds=true
  videoLayer.videoGravity = .resizeAspectFill;layer.addSublayer(videoLayer)
  let center=NotificationCenter.default
  observers.append(center.addObserver(forName:UIApplication.willResignActiveNotification,object:nil,queue:.main){[weak self]_ in self?.pause()})
  observers.append(center.addObserver(forName:UIApplication.didBecomeActiveNotification,object:nil,queue:.main){[weak self]_ in self?.startIfNeeded()})
 }
 override func layoutSubviews(){super.layoutSubviews();CATransaction.begin();CATransaction.setDisableActions(true);videoLayer.frame=bounds;CATransaction.commit()}
 override func didMoveToWindow(){super.didMoveToWindow();if window==nil{pause()}else{startIfNeeded()}}
 private func startIfNeeded() {
  guard window != nil,UIApplication.shared.applicationState == .active,let url=URL(string:sourceUri),url.isFileURL else{return}
  if let player {if active && presented {player.playImmediately(atRate:1)};return}
   guard !starting else{return}
  starting=true;let token=generation,uri=sourceUri
  DispatchQueue.global(qos:.userInitiated).async { [weak self] in
   let prepared=MotionArtworkSource(url:url)
   DispatchQueue.main.async { [weak self] in
    guard let self,self.generation==token,self.window != nil,UIApplication.shared.applicationState == .active else{prepared?.close();return}
    self.starting=false
    guard let prepared else {self.onStatus(["uri":uri,"ready":false,"unavailable":true]);return}
    let item=AVPlayerItem(asset:prepared.asset),player=AVQueuePlayer()
    if self.blurred {item.videoComposition=AVVideoComposition(asset:prepared.asset){request in
     let source=request.sourceImage.clampedToExtent()
     let filter=CIFilter(name:"CIGaussianBlur",parameters:[kCIInputRadiusKey:36])!
     filter.setValue(source,forKey:kCIInputImageKey)
     guard let output=filter.outputImage else {request.finish(with:source,context:nil);return}
     request.finish(with:output.cropped(to:request.sourceImage.extent),context:nil)
    }}
    self.source=prepared;self.player=player;player.isMuted=true;player.volume=0
    player.automaticallyWaitsToMinimizeStalling=false
    player.allowsExternalPlayback=false;player.preventsDisplaySleepDuringVideoPlayback=false
    self.loop=AVPlayerLooper(player:player,templateItem:item);self.videoLayer.player=player
    self.ready=self.videoLayer.observe(\.isReadyForDisplay,options:[.initial,.new]) { [weak self] layer,_ in
     DispatchQueue.main.async { [weak self] in guard let self,self.generation==token else{return}
      if layer.isReadyForDisplay {
       if !self.active || !self.presented {player.pause()}
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
  player?.pause();loop?.disableLooping();loop=nil;player?.removeAllItems();videoLayer.player=nil;player=nil
  source?.close();source=nil
 }
 private func pause() {player?.pause()}
 private func rewind() {
  guard let player else{return}
  let token=generation
  player.pause()
  player.seek(to:.zero,toleranceBefore:.zero,toleranceAfter:.zero){[weak self] finished in
   DispatchQueue.main.async{[weak self] in
    guard let self,finished,self.generation==token,self.active,self.presented,self.window != nil,UIApplication.shared.applicationState == .active else{return}
    player.playImmediately(atRate:1)
   }
  }
 }
 deinit {for observer in observers{NotificationCenter.default.removeObserver(observer)};player?.pause();loop?.disableLooping();source?.close()}
}
