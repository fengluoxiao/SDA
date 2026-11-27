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
 // Repeat in the reverse leg: resume must restart its timer and wait for a
 // real frame after idle memory is released, without rewinding the cover.
 view.testReverse(at:2)
 try await waitFor("Reverse leg did not start"){view.testReversePosition<1.9}
 let recreated=SdaMotionArtworkView()
 recreated.bounds=view.bounds;recreated.blurred=true;recreated.window=window
 recreated.presented=true;recreated.active=true;recreated.sourceUri=url.absoluteString
 try await waitFor("Recreated background started an independent reverse timeline"){
  recreated.testDirection == view.testDirection && recreated.testReverseFrameTime>0 && recreated.testReportedReady && abs(recreated.testReverseFrameTime-view.testReverseFrameTime)<0.07
 }
 recreated.active=false;recreated.testStop()
 view.active=false
 let frozen=view.testReversePosition
 try require(view.testRetainedBytes==0,"Paused view retained reverse working windows")
 view.bounds=CGRect(x:0,y:0,width:932,height:430);view.layoutSubviews()
 try await Task.sleep(nanoseconds:50_000_000)
 try require(abs(view.testReversePosition-frozen)<0.001,"Hidden reverse cover advanced during settings")
 view.active=true
 view.bounds=CGRect(x:0,y:0,width:430,height:932);view.layoutSubviews()
 try await waitFor("Rotation left the reverse timer paused"){view.testReversePosition<frozen-0.05}
 UIApplication.shared.applicationState = .background
 NotificationCenter.default.post(name:UIApplication.willResignActiveNotification,object:nil)
 view.layoutSubviews()
 let backgroundPosition=view.testReversePosition
 try await Task.sleep(nanoseconds:50_000_000)
 try require(abs(view.testReversePosition-backgroundPosition)<0.001,"Background layout resumed the cover")
 UIApplication.shared.applicationState = .active
 NotificationCenter.default.post(name:UIApplication.didBecomeActiveNotification,object:nil)
 try await waitFor("Foreground restoration did not resume the reverse cover"){view.testReversePosition<backgroundPosition-0.05}
 print("{\"motionViewLifecycle\":true,\"detachedPreparationRecovered\":true,\"settingsAndStyleRotations\":3,\"forwardAndReverseResumed\":true,\"recreatedBlurSharesSharpTimeline\":true,\"pausedWindowsReleased\":true,\"physicalDeviceTested\":false}")
}
