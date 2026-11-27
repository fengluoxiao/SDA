import UIKit
import AVFoundation

// Real UIKit host for the production view; only the Expo module registration
// is replaced. No synthetic UIWindow/UIApplication is used in this target.
final class AppContext {}
class ExpoView:UIView {
 required init(appContext:AppContext?=nil){super.init(frame:.zero)}
 @available(*,unavailable) required init?(coder:NSCoder){fatalError()}
}
final class EventDispatcher {func callAsFunction(_ value:[String:Any]){}}

enum MotionUIResult {
 static var directory:URL{FileManager.default.urls(for:.documentDirectory,in:.userDomainMask)[0]}
 static func write(_ name:String,_ value:[String:Any]){
  try? FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
  if let data=try? JSONSerialization.data(withJSONObject:value,options:.sortedKeys){try? data.write(to:directory.appendingPathComponent(name),options:.atomic)}
 }
}
@objc(MotionArtworkTestDelegate) final class MotionArtworkTestDelegate:UIResponder,UIApplicationDelegate {
 func application(_ application:UIApplication,didFinishLaunchingWithOptions options:[UIApplication.LaunchOptionsKey:Any]?=nil)->Bool {
  DispatchQueue.global().asyncAfter(deadline:.now()+480){MotionUIResult.write("result.json",["passed":false,"error":"UIKit motion test watchdog expired"]);exit(1)}
  return true
 }
}
@objc(MotionArtworkTestScene) final class MotionArtworkTestScene:UIResponder,UIWindowSceneDelegate {
 var window:UIWindow?
 func scene(_ scene:UIScene,willConnectTo session:UISceneSession,options:UIScene.ConnectionOptions){
  guard let scene=scene as? UIWindowScene else{return}
  let window=UIWindow(windowScene:scene),navigation=UINavigationController(rootViewController:MotionArtworkTestController())
  navigation.setNavigationBarHidden(true,animated:false);window.rootViewController=navigation
  self.window=window;window.makeKeyAndVisible()
 }
}
final class MotionArtworkTestController:UIViewController {
 private let sharp=SdaMotionArtworkView()
 private var backdrop:SdaMotionArtworkView?
 private var tabletStyle=true,started=false,requestID=0,rotations=0,settingsReturns=0
 private var captures=[[String:Any]]()
 override var supportedInterfaceOrientations:UIInterfaceOrientationMask{.allButUpsideDown}
 override var prefersStatusBarHidden:Bool{true}
 override func viewDidLoad(){
  super.viewDidLoad();view.backgroundColor=UIColor(red:0.19,green:0.19,blue:0.20,alpha:1)
  view.addSubview(sharp);addBackdrop()
  sharp.sourceUri=Bundle.main.url(forResource:"motion",withExtension:"m4a")!.absoluteString
  sharp.presented=true;sharp.active=true
 }
 override func viewDidAppear(_ animated:Bool){
  super.viewDidAppear(animated);sharp.active=true;backdrop?.active=true
  guard !started else{return};started=true
  Task{@MainActor in do{try await run()}catch{finish(false,String(describing:error))}}
 }
 override func viewWillDisappear(_ animated:Bool){
  super.viewWillDisappear(animated);sharp.active=false;backdrop?.active=false
 }
 private func addBackdrop(){
  let background=SdaMotionArtworkView();backdrop=background;view.insertSubview(background,at:0)
  background.blurred=true;background.bottomExtension=true;background.presented=true;background.active=true
  background.sourceUri=Bundle.main.url(forResource:"motion",withExtension:"m4a")!.absoluteString
 }
 override func viewDidLayoutSubviews(){
  super.viewDidLayoutSubviews();let size=view.bounds.size,landscape=size.width>size.height
  let coverWidth=landscape ? size.height+48:size.width
  let coverHeight=landscape ? coverWidth:coverWidth/0.5625
  sharp.frame=CGRect(x:landscape ? -24:0,y:0,width:coverWidth,height:coverHeight)
  sharp.featherMode=landscape ? (tabletStyle ? "phone-landscape":"none"):"portrait"
  sharp.featherHeight=Double(landscape ? coverHeight:min(coverHeight,size.height*0.6))
  backdrop?.frame=view.bounds;backdrop?.bottomExtension = !landscape
 }
 @MainActor private func wait(_ reason:String,timeout:Double=15,_ condition:()->Bool)async throws {
  let deadline=Date().addingTimeInterval(timeout)
  while !condition(){if Date()>deadline{throw NSError(domain:reason,code:1)};try await Task.sleep(nanoseconds:25_000_000)}
 }
 @MainActor private func rotate(_ landscape:Bool)async throws {
  guard let scene=view.window?.windowScene else{throw NSError(domain:"Missing real UIWindowScene",code:1)}
  setNeedsUpdateOfSupportedInterfaceOrientations()
  scene.requestGeometryUpdate(.iOS(interfaceOrientations:landscape ? .landscapeRight:.portrait))
  try await wait("Window did not rotate"){(self.view.bounds.width>self.view.bounds.height) == landscape}
  rotations+=1
  try await Task.sleep(nanoseconds:250_000_000)
 }
 @MainActor private func screenshot(_ label:String)async throws->Double {
  requestID+=1;let id=requestID
  guard let window=view.window else{throw NSError(domain:"Missing window for screen sample",code:1)}
  let frame=sharp.convert(sharp.bounds,to:window),size=window.bounds.size
  let rect=[(frame.minX+frame.width*0.2)/size.width,(frame.minY+frame.height*0.18)/size.height,frame.width*0.20/size.width,frame.height*0.12/size.height]
  MotionUIResult.write("request.json",["id":id,"label":label,"rect":rect,"playback":sharp.testState])
  var difference=0.0
  try await wait("Screen capture acknowledgement missing",timeout:60){
   guard let data=try? Data(contentsOf:MotionUIResult.directory.appendingPathComponent("ack.json")),let ack=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any],ack["id"] as? Int==id else{return false}
   difference=ack["difference"] as? Double ?? 0;return true
  }
  return difference
 }
 @MainActor private func movingScreen(_ label:String)async throws {
  // Stay away from either ping-pong endpoint, where equal before/after
  // positions can legitimately display the same frame.
  try await wait("Motion clock stopped before screen verification"){
   let position=self.sharp.testPlaybackPosition
   return self.sharp.testDirection>0 ? position>0.5 && position<1.8:position>3.2 && position<4.5
  }
  _=try await screenshot(label+"-before")
  try await Task.sleep(nanoseconds:650_000_000)
  var difference=try await screenshot(label+"-after")
  // A slow first simulator capture can span a complete ping-pong period.
  // One additional screen sample rejects that alias; a frozen view still fails.
  if difference<=1{try await Task.sleep(nanoseconds:450_000_000);difference=max(difference,try await screenshot(label+"-recheck-after"))}
  guard difference>1 else{throw NSError(domain:"Visible artwork froze at "+label,code:1)}
  captures.append(["phase":label,"meanScreenDifference":difference])
 }
 @MainActor private func run()async throws {
  try await wait("Screen harness did not attach",timeout:180){FileManager.default.fileExists(atPath:MotionUIResult.directory.appendingPathComponent("harness-ready").path)}
  try await wait("Real UIKit motion player was not ready"){self.sharp.testPlayer?.currentItem?.status == .readyToPlay && UIApplication.shared.applicationState == .active}
  let player=sharp.testPlayer
  try await movingScreen("tablet-portrait")
  try await visitSettings(nil);try await movingScreen("settings-unchanged-first-return")
  try await visitSettings(nil);try await movingScreen("settings-unchanged-repeat-return")
  try await rotate(true);try await movingScreen("tablet-landscape")
  try await rotate(false);try await movingScreen("tablet-portrait-return")
  try await visitSettings(false)
  try await movingScreen("style-disabled-return")
  try await rotate(true);try await movingScreen("disabled-landscape")
  try await rotate(false);try await movingScreen("disabled-portrait")
  try await visitSettings(true)
  try await movingScreen("style-re-enabled-return")
  try await rotate(true);try await movingScreen("re-enabled-landscape")
  try await rotate(false);try await movingScreen("re-enabled-portrait")
  try await visitSettings(nil);try await movingScreen("extra-settings-after-re-enabled")
  guard sharp.testPlayer === player else{throw NSError(domain:"Rotation replaced the selected player",code:1)}
  sharp.testReverse(at:4.4);try await movingScreen("reverse-after-rotation")
  try await visitSettings(nil);try await movingScreen("settings-unchanged-reverse-return")
  sharp.active=false;try await Task.sleep(nanoseconds:200_000_000)
  _=try await screenshot("paused-before");try await Task.sleep(nanoseconds:650_000_000)
  let pausedDifference=try await screenshot("paused-after")
  guard pausedDifference<0.5 else{throw NSError(domain:"Artwork moved while paused",code:1)}
  sharp.active=true;try await movingScreen("manual-resume")
  finish(true,nil)
 }
 @MainActor private func visitSettings(_ enabled:Bool?)async throws {
  guard let navigation=navigationController else{throw NSError(domain:"Missing production-style navigation stack",code:1)}
  sharp.active=false;backdrop?.active=false
  let settings=UIViewController();settings.view.backgroundColor = .black
  navigation.pushViewController(settings,animated:true)
  try await wait("Settings push did not finish"){navigation.topViewController === settings && settings.view.window != nil && navigation.transitionCoordinator==nil}
  if let enabled{
   tabletStyle=enabled;backdrop?.testStop();backdrop?.removeFromSuperview();backdrop=nil
   if enabled{addBackdrop();backdrop?.active=false}
  }
  // React clears `settings` at pop start. Media remains inactive until the
  // home's viewDidAppear/onAppear callback, matching IOSSettingsNavigation.
  navigation.popViewController(animated:true)
  try await wait("Settings pop did not finish"){navigation.topViewController === self && self.view.window != nil && navigation.transitionCoordinator==nil}
  view.setNeedsLayout();view.layoutIfNeeded();settingsReturns+=1
  guard sharp.active else{throw NSError(domain:"Home appearance did not resume motion",code:1)}
 }
 private func finish(_ passed:Bool,_ error:String?){
  let playback=sharp.testState
  sharp.testStop();backdrop?.testStop()
  var result:[String:Any]=["passed":passed,"realUIKitWindow":true,"nativeNavigationPushPop":true,"settingsReturns":settingsReturns,"realOrientationChanges":rotations,"styleSwitches":[false,true],"visibleMotionChecks":captures,"screenshots":requestID,"playback":playback,"physicalDeviceTested":false]
  if let error{result["error"]=error}
  MotionUIResult.write("result.json",result);exit(passed ? 0:1)
 }
}
@main struct MotionArtworkUITest {
 static func main(){UIApplicationMain(CommandLine.argc,CommandLine.unsafeArgv,nil,NSStringFromClass(MotionArtworkTestDelegate.self))}
}
