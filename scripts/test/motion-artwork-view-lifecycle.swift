import Foundation
import AVFoundation

@MainActor func testMotionArtworkViewLifecycle(_ url:URL)async throws {
 func require(_ value:Bool,_ reason:String)throws{if !value{throw NSError(domain:reason,code:1)}}
 func waitFor(_ reason:String,_ condition:()->Bool)async throws {
  let deadline=Date().addingTimeInterval(10)
  while !condition(){try require(Date()<deadline,reason);try await Task.sleep(nanoseconds:10_000_000)}
 }
 let view=SdaMotionArtworkView(),window=NSObject()
 defer{view.testStop();UIApplication.shared.applicationState = .active}
 view.bounds=CGRect(x:0,y:0,width:430,height:573)
 view.window=window;view.presented=true;view.active=true;view.sourceUri=url.absoluteString
 try require(view.testStarting,"The real source preparation did not start")
 // The main completion cannot run before the window is removed here. This
 // reproduces preparation completing during a mask's transient detachment.
 view.window=nil;view.didMoveToWindow()
 try await waitFor("Detached preparation left the view permanently starting"){!view.testStarting}
 try require(view.testPlayer==nil,"Detached preparation installed an invisible player")
 view.window=window;view.didMoveToWindow()
 try await waitFor("Reattachment could not prepare the motion player"){view.testPlayer?.currentItem?.status == .readyToPlay}
 guard let original=view.testPlayer else{throw NSError(domain:"Motion player missing",code:1)}
 try await waitFor("Reattached motion player did not resume"){original.rate>0}
 for _ in 0..<3 {
  // Settings pauses both surfaces; disabling tablet style removes only the
  // blurred surface. Keep the sharp player's identity through later rotations.
  let background=SdaMotionArtworkView()
  background.bounds=view.bounds;background.blurred=true;background.window=window
  background.presented=true;background.active=true;background.sourceUri=url.absoluteString
  try await waitFor("Blurred surface failed to prepare"){background.testPlayer != nil}
  view.active=false;background.active=false;background.testStop()
  original.pause();view.layoutSubviews()
  try require(original.rate==0,"Layout resumed artwork while settings were open")
  view.active=true
  for size in [CGSize(width:932,height:430),CGSize(width:430,height:932)] {
   original.pause();view.bounds=CGRect(origin:.zero,size:size);view.layoutSubviews()
   try await waitFor("Rotation left the visible forward cover paused"){original.rate>0}
   try require(view.testPlayer === original,"Rotation recreated the selected cover player")
  }
 }
 // Exercise the real native loop before recreating its blurred follower.
 let loops=view.testLoopCount
 try await waitFor("Forward loop did not complete"){view.testLoopCount>loops}
 try require(original.rate>0,"Native loop did not continue forward")
 let recreated=SdaMotionArtworkView()
 recreated.bounds=view.bounds;recreated.blurred=true;recreated.window=window
 recreated.presented=true;recreated.active=true;recreated.sourceUri=url.absoluteString
 try await waitFor("Recreated background did not follow the sharp cover"){
  recreated.testPlayer?.currentItem?.status == .readyToPlay && abs(recreated.testLocalPosition-view.testPlaybackPosition)<0.15
 }
 recreated.active=false;recreated.testStop()
 view.active=false
 let frozen=view.testPlaybackPosition
 view.bounds=CGRect(x:0,y:0,width:932,height:430);view.layoutSubviews()
 try await Task.sleep(nanoseconds:50_000_000)
 try require(abs(view.testPlaybackPosition-frozen)<0.03,"Hidden cover advanced during settings")
 view.active=true
 view.bounds=CGRect(x:0,y:0,width:430,height:932);view.layoutSubviews()
 try await waitFor("Rotation left the forward loop paused"){original.rate>0}
 UIApplication.shared.applicationState = .background
 NotificationCenter.default.post(name:UIApplication.willResignActiveNotification,object:nil)
 view.layoutSubviews()
 let backgroundPosition=view.testPlaybackPosition
 try await Task.sleep(nanoseconds:50_000_000)
 try require(abs(view.testPlaybackPosition-backgroundPosition)<0.03,"Background layout resumed the cover")
 UIApplication.shared.applicationState = .active
 NotificationCenter.default.post(name:UIApplication.didBecomeActiveNotification,object:nil)
 try await waitFor("Foreground restoration did not resume the forward cover"){original.rate>0}
 print("{\"motionViewLifecycle\":true,\"detachedPreparationRecovered\":true,\"settingsAndStyleRotations\":3,\"forwardLoopResumed\":true,\"recreatedBlurSharesSharpTimeline\":true,\"nativeForwardLoopVerified\":true,\"physicalDeviceTested\":false}")
}
