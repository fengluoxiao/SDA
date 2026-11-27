import Foundation
import QuartzCore
import ImageIO

func testMotionArtworkMask(_ png:URL)throws {
 func require(_ value:Bool,_ message:String)throws{if !value{throw NSError(domain:message,code:1)}}
 guard let source=CGImageSourceCreateWithURL(png as CFURL,nil),let image=CGImageSourceCreateImageAtIndex(source,0,nil) else{throw NSError(domain:"Missing approved feather PNG",code:1)}
 var pixels=[UInt8](repeating:0,count:image.width*image.height*4)
 try pixels.withUnsafeMutableBytes{storage in
  guard let context=CGContext(data:storage.baseAddress,width:image.width,height:image.height,bitsPerComponent:8,bytesPerRow:image.width*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else{throw NSError(domain:"Feather pixel context missing",code:1)}
  context.draw(image,in:CGRect(x:0,y:0,width:CGFloat(image.width),height:CGFloat(image.height)))
 }
 let flipped=pixels[3]>pixels[(image.height-1)*image.width*4+3]
 let mask=MotionArtworkMask();mask.update(size:CGSize(width:430,height:573),mode:"portrait",height:559)
 let vertical=mask.layer.sublayers![0] as! CAGradientLayer
 let colors=vertical.colors as! [CGColor]
 for step in 0...32 {
  var row=Int((1-Double(step)/32)*Double(image.height-1)).clamped(to:0...(image.height-1))
  if flipped{row=image.height-1-row}
  let alpha=Double(pixels[row*image.width*4+3])/255
  try require(abs(Double(colors[step+1].alpha)-alpha)<0.008,"Native feather differs from the approved PNG alpha curve")
 }
 try require(abs(vertical.locations!.last!.doubleValue-1)<0.00001,"Bottom mask must end transparent")
 for mode in ["portrait","phone-landscape","tablet-landscape","none"]{
  mask.update(size:CGSize(width:430,height:573),mode:mode,height:559)
  for gradient in [vertical,mask.layer.mask as! CAGradientLayer]{
   let values=gradient.locations!.map(\.doubleValue)
   try require(values==values.sorted() && values.allSatisfy{$0>=0 && $0<=1},"Rotation generated an invalid native mask")
  }
 }
 print("{\"nativeFeatherMatchesApprovedPNG\":true,\"nativeMaskModes\":4}")
}
private extension Int {func clamped(to range:ClosedRange<Int>)->Int{Swift.min(range.upperBound,Swift.max(range.lowerBound,self))}}
