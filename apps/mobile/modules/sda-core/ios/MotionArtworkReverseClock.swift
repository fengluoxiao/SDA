import Foundation

struct MotionArtworkReverseClock {
 private(set) var position:Double=0
 private var anchor:Double?
 mutating func begin(at position:Double){self.position=max(0,position);anchor=nil}
 mutating func resume(at now:Double){if anchor==nil{anchor=now}}
 func current(at now:Double)->Double{max(0,position-max(0,now-(anchor ?? now)))}
 mutating func pause(at now:Double){position=current(at:now);anchor=nil}
}
