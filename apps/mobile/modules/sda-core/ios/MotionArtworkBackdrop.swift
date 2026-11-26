import CoreImage
import CoreGraphics
import Foundation

final class MotionArtworkBackdrop: @unchecked Sendable {
 static let bottomFraction:CGFloat=0.28
 private let lock=NSLock()
 private var extended=false
 var bottomExtension:Bool {
  get{lock.lock();defer{lock.unlock()};return extended}
  set{lock.lock();extended=newValue;lock.unlock()}
 }
 func render(_ image:CIImage)->CIImage{Self.blurred(image,bottomExtension:bottomExtension,radius:36)}
 static func bottomBand(_ extent:CGRect)->CGRect{CGRect(x:extent.minX,y:extent.minY,width:extent.width,height:extent.height*bottomFraction)}
 static func extendedImage(_ image:CIImage)->CIImage {
  let band=bottomBand(image.extent)
  let cropped=image.cropped(to:band)
  return cropped.transformed(by:CGAffineTransform(translationX:-band.minX,y:-band.minY))
   .transformed(by:CGAffineTransform(scaleX:1,y:1/bottomFraction))
   .transformed(by:CGAffineTransform(translationX:image.extent.minX,y:image.extent.minY))
 }
 static func blurred(_ image:CIImage,bottomExtension:Bool,radius:CGFloat)->CIImage {
  let source=bottomExtension ? extendedImage(image):image
  var result=source.clampedToExtent().applyingFilter("CIGaussianBlur",parameters:[kCIInputRadiusKey:radius]).cropped(to:image.extent)
  if bottomExtension{
   result=result.applyingFilter("CIColorControls",parameters:[kCIInputSaturationKey:0.78])
   let average=image.applyingFilter("CIAreaAverage",parameters:[kCIInputExtentKey:CIVector(cgRect:bottomBand(image.extent))])
   let tone=average.applyingFilter("CIColorMatrix",parameters:["inputRVector":CIVector(x:0.5,y:0,z:0,w:0),"inputGVector":CIVector(x:0,y:0.5,z:0,w:0),"inputBVector":CIVector(x:0,y:0,z:0.5,w:0),"inputBiasVector":CIVector(x:0.0052,y:0.0052,z:0.0052,w:0)])
    .applyingFilter("CIColorControls",parameters:[kCIInputSaturationKey:0.78]).clampedToExtent().cropped(to:image.extent)
   let mask=CIFilter(name:"CILinearGradient",parameters:["inputPoint0":CIVector(x:image.extent.midX,y:image.extent.minY),"inputPoint1":CIVector(x:image.extent.midX,y:image.extent.minY+image.extent.height*0.4),"inputColor0":CIColor(red:0.85,green:0.85,blue:0.85),"inputColor1":CIColor(red:0,green:0,blue:0)])!.outputImage!.cropped(to:image.extent)
   result=tone.applyingFilter("CIBlendWithMask",parameters:[kCIInputBackgroundImageKey:result,kCIInputMaskImageKey:mask])
    .applyingFilter("CIColorClamp",parameters:["inputMaxComponents":CIVector(x:0.30,y:0.30,z:0.30,w:1)]).cropped(to:image.extent)
  }
  return result
 }
}
