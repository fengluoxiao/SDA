import Foundation
import QuartzCore

// Only the Expo/UIKit host is replaced. The generated view under test is the
// production class, using real AVPlayer, layers, source reads and reverse frames.
final class AppContext {}
class ExpoView:NSObject {
 var layer=CALayer(),bounds=CGRect.zero
 var window:NSObject?
 var clipsToBounds=false,isUserInteractionEnabled=true
 required init(appContext:AppContext?=nil){super.init()}
 func layoutSubviews(){}
 func didMoveToWindow(){}
}
final class EventDispatcher {func callAsFunction(_ value:[String:Any]){}}
final class UIApplication {
 enum State {case active,inactive,background}
 static let shared=UIApplication()
 static let willResignActiveNotification=Notification.Name("motion-test-inactive")
 static let didBecomeActiveNotification=Notification.Name("motion-test-active")
 var applicationState=State.active
}
