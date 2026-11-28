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
  if bottomExtension{result=extendedImage(result)}
  return result
 }
}
