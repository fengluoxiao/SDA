import Foundation
import QuartzCore
import CoreGraphics

// Keep live video and reverse images in one UIKit/Core Animation layer tree.
// The alpha curve is the smoothstep ramp used by artwork-color-fade.png.
final class MotionArtworkMask {
 let layer=CALayer()
 private let vertical=CAGradientLayer(),horizontal=CAGradientLayer()
 init(){
  vertical.startPoint=CGPoint(x:0.5,y:0);vertical.endPoint=CGPoint(x:0.5,y:1)
  horizontal.startPoint=CGPoint(x:0,y:0.5);horizontal.endPoint=CGPoint(x:1,y:0.5)
  layer.addSublayer(vertical);layer.mask=horizontal
 }
 func update(size:CGSize,mode:String,height:Double){
  CATransaction.begin();CATransaction.setDisableActions(true);defer{CATransaction.commit()}
  layer.frame=CGRect(origin:.zero,size:size);vertical.frame=layer.bounds;horizontal.frame=layer.bounds
  let safeHeight=height.isFinite ? max(0,CGFloat(height)):size.height
  let boundary=mode=="portrait" ? min(size.height,safeHeight):size.height
  let bottomSpan=mode=="portrait" ? boundary*0.30:mode=="tablet-landscape" ? size.height*0.35:0
  let rightSpan=mode=="phone-landscape" ? size.width*0.30:mode=="tablet-landscape" ? min(48,size.width*0.10):0
  ramp(vertical,start:boundary-bottomSpan,end:boundary,length:size.height,enabled:bottomSpan>0)
  ramp(horizontal,start:size.width-rightSpan,end:size.width,length:size.width,enabled:rightSpan>0)
 }
 private func ramp(_ gradient:CAGradientLayer,start:CGFloat,end:CGFloat,length:CGFloat,enabled:Bool){
  let white=CGColor(gray:1,alpha:1)
  guard enabled,length>0 else{gradient.colors=[white,white];gradient.locations=[0,1];return}
  var colors=[CGColor](),locations=[NSNumber]()
  colors.append(white);locations.append(0)
  for step in 0...32 {
   let t=Double(step)/32,alpha=1-t*t*(3-2*t)
   colors.append(CGColor(gray:1,alpha:alpha))
   locations.append(NSNumber(value:Double((start+(end-start)*CGFloat(t))/length)))
  }
  colors.append(CGColor(gray:1,alpha:0));locations.append(1)
  gradient.colors=colors;gradient.locations=locations
 }
}
