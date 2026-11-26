import CoreImage
import CoreGraphics
import Foundation

final class MotionArtworkBackdrop: @unchecked Sendable {
 static let bottomFraction:CGFloat=0.28
 private let lock=NSLock()
 private static let paletteContext=CIContext(options:[.cacheIntermediates:false])
 private static let linearSpace=CGColorSpace(name:CGColorSpace.extendedLinearSRGB)!
 private var extended=false
 var bottomExtension:Bool {
  get{lock.lock();defer{lock.unlock()};return extended}
  set{lock.lock();extended=newValue;lock.unlock()}
 }
 func render(_ image:CIImage)->CIImage{Self.blurred(image,bottomExtension:bottomExtension,radius:36*min(1,image.extent.width/720))}
 static func bottomBand(_ extent:CGRect)->CGRect{CGRect(x:extent.minX,y:extent.minY,width:extent.width,height:extent.height*bottomFraction)}
 static func extendedImage(_ image:CIImage)->CIImage {
  let band=bottomBand(image.extent)
  let cropped=image.cropped(to:band)
  return cropped.transformed(by:CGAffineTransform(translationX:-band.minX,y:-band.minY))
   .transformed(by:CGAffineTransform(scaleX:1,y:1/bottomFraction))
   .transformed(by:CGAffineTransform(translationX:image.extent.minX,y:image.extent.minY))
 }
 static func blurred(_ image:CIImage,bottomExtension:Bool,radius:CGFloat)->CIImage {
  var result=image.clampedToExtent().applyingFilter("CIGaussianBlur",parameters:[kCIInputRadiusKey:radius]).cropped(to:image.extent)
  if bottomExtension{result=continuationStyle(extendedImage(result))}
  return result
 }
 static func continuationStyle(_ image:CIImage)->CIImage {
  image.applyingFilter("CIColorControls",parameters:[kCIInputSaturationKey:0.78])
   .applyingFilter("CIColorClamp",parameters:["inputMaxComponents":CIVector(x:0.30,y:0.30,z:0.30,w:1)])
 }
 private static func tone(_ red:Double,_ green:Double,_ blue:Double)->CGColor {
  let values=[red,green,blue].map{$0*0.5+0.0052}
  let luminance=values[0]*0.2126+values[1]*0.7152+values[2]*0.0722
  let components=values.map{CGFloat(min(0.30,max(0,$0*0.78+luminance*0.22)))}+[CGFloat(0.85)]
  return CGColor(colorSpace:linearSpace,components:components)!
 }
 static func toneColor(_ image:CIImage)->CGColor {
  let average=image.applyingFilter("CIAreaAverage",parameters:[kCIInputExtentKey:CIVector(cgRect:bottomBand(image.extent))])
  var pixel=[Float](repeating:0,count:4)
  pixel.withUnsafeMutableBytes{storage in paletteContext.render(average,toBitmap:storage.baseAddress!,rowBytes:16,bounds:CGRect(x:0,y:0,width:1,height:1),format:.RGBAf,colorSpace:linearSpace)}
  return tone(Double(pixel[0]),Double(pixel[1]),Double(pixel[2]))
 }
 static func toneColor(_ image:CGImage)->CGColor {
  guard let data=image.dataProvider?.data,let pixels=CFDataGetBytePtr(data) else{return tone(0,0,0)}
  var red=0.0,green=0.0,blue=0.0
  func linear(_ value:UInt8)->Double{let unit=Double(value)/255;return unit<=0.04045 ? unit/12.92:pow((unit+0.055)/1.055,2.4)}
  for row in 0..<8{for column in 0..<16{
   let x=min(image.width-1,Int((Double(column)+0.5)/16*Double(image.width)))
   let y=min(image.height-1,Int((0.72+(Double(row)+0.5)/8*0.28)*Double(image.height)))
   let at=y*image.bytesPerRow+x*4
   red+=linear(pixels[at])/128;green+=linear(pixels[at+1])/128;blue+=linear(pixels[at+2])/128
  }}
  return tone(red,green,blue)
 }
}
