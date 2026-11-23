import ExpoModulesCore
import AVFoundation
import UIKit

public final class SdaMotionArtworkModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaMotionArtwork")
  View(SdaMotionArtworkView.self) {
   Events("onStatus")
   Prop("sourceUri") { (view:SdaMotionArtworkView, value:String) in view.sourceUri=value }
   Prop("active") { (view:SdaMotionArtworkView, value:Bool) in view.active=value }
  }
 }
}
final class SdaMotionArtworkView: ExpoView {
 let onStatus=EventDispatcher()
 var sourceUri="" {didSet{if sourceUri != oldValue {stop();startIfNeeded()}}}
 var active=false {didSet{if active {startIfNeeded()}else{pause()}}}
 private let videoLayer=AVPlayerLayer()
 private var source:MotionArtworkSource?, player:AVQueuePlayer?, loop:AVPlayerLooper?
 private var ready:NSKeyValueObservation?, failure:NSKeyValueObservation?, observers=[NSObjectProtocol]()
 private var generation=0,starting=false
 required init(appContext:AppContext? = nil) {
  super.init(appContext:appContext);clipsToBounds=true;isUserInteractionEnabled=false
  videoLayer.videoGravity = .resizeAspectFill;layer.addSublayer(videoLayer)
  let center=NotificationCenter.default
  observers.append(center.addObserver(forName:UIApplication.willResignActiveNotification,object:nil,queue:.main){[weak self]_ in self?.pause()})
  observers.append(center.addObserver(forName:UIApplication.didBecomeActiveNotification,object:nil,queue:.main){[weak self]_ in self?.startIfNeeded()})
 }
 override func layoutSubviews(){super.layoutSubviews();CATransaction.begin();CATransaction.setDisableActions(true);videoLayer.frame=bounds;CATransaction.commit()}
 override func didMoveToWindow(){super.didMoveToWindow();if window==nil{pause()}else{startIfNeeded()}}
 private func startIfNeeded() {
  guard window != nil,UIApplication.shared.applicationState == .active,let url=URL(string:sourceUri),url.isFileURL else{return}
  if let player {if active {player.play()};return}
  guard active,!starting else{return}
  starting=true;let token=generation,uri=sourceUri
  DispatchQueue.global(qos:.utility).async { [weak self] in
   let prepared=MotionArtworkSource(url:url)
   DispatchQueue.main.async { [weak self] in
    guard let self,self.generation==token,self.active,self.window != nil,UIApplication.shared.applicationState == .active else{prepared?.close();return}
    self.starting=false
    guard let prepared else {self.onStatus(["uri":uri,"ready":false,"unavailable":true]);return}
    let item=AVPlayerItem(asset:prepared.asset),player=AVQueuePlayer()
    self.source=prepared;self.player=player;player.isMuted=true;player.volume=0
    player.allowsExternalPlayback=false;player.preventsDisplaySleepDuringVideoPlayback=false
    self.loop=AVPlayerLooper(player:player,templateItem:item);self.videoLayer.player=player
    self.ready=self.videoLayer.observe(\.isReadyForDisplay,options:[.initial,.new]) { [weak self] layer,_ in
     DispatchQueue.main.async { [weak self] in guard let self,self.generation==token else{return}
      if layer.isReadyForDisplay {self.onStatus(["uri":uri,"ready":true])}
     }
    }
    self.failure=player.observe(\.status,options:[.new]) { [weak self] player,_ in
     if player.status == .failed {DispatchQueue.main.async { [weak self] in guard let self,self.generation==token else{return};self.stop();self.onStatus(["uri":uri,"ready":false,"unavailable":true])}}
    }
    player.play()
   }
  }
 }
 private func stop() {
  generation+=1;starting=false;ready?.invalidate();ready=nil;failure?.invalidate();failure=nil
  player?.pause();loop?.disableLooping();loop=nil;player?.removeAllItems();videoLayer.player=nil;player=nil
  source?.close();source=nil
 }
 private func pause() {player?.pause()}
 deinit {for observer in observers{NotificationCenter.default.removeObserver(observer)};player?.pause();loop?.disableLooping();source?.close()}
}
