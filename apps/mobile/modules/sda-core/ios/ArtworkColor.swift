import Foundation
import CoreGraphics
import ImageIO

// Optional artwork metadata only. No GL context or playback session is needed.
enum ArtworkColor {
 static func bottomBand(_ data: Data) -> String? {
  guard let source = CGImageSourceCreateWithData(data as CFData, nil),
   let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
    kCGImageSourceCreateThumbnailFromImageAlways: true,
    kCGImageSourceCreateThumbnailWithTransform: true,
    kCGImageSourceThumbnailMaxPixelSize: 128
   ] as CFDictionary) else { return nil }
  let top = min(image.height - 1, Int(Double(image.height) * 0.70))
  let bandHeight = min(image.height - top, max(1, Int(Double(image.height) * 0.29)))
  guard let band = image.cropping(to: CGRect(x: 0, y: CGFloat(top), width: CGFloat(image.width), height: CGFloat(bandHeight))) else { return nil }
  var pixels = [UInt8](repeating: 0, count: 16 * 16 * 4)
  let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
   guard let context = CGContext(data: buffer.baseAddress, width: 16, height: 16,
    bitsPerComponent: 8, bytesPerRow: 16 * 4,
    space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else { return false }
   context.interpolationQuality = .high
   context.draw(band, in: CGRect(x: 0, y: 0, width: 16, height: 16))
   return true
  }
  guard drawn else { return nil }
  var sums = [Double](repeating: 0, count: 3), weight = 0.0
  for i in stride(from: 0, to: pixels.count, by: 4) {
   weight += Double(pixels[i + 3]) / 255.0
   // CGContext supplies premultiplied components; don't multiply alpha twice.
   for channel in 0..<3 { sums[channel] += Double(pixels[i + channel]) }
  }
  guard weight >= 1 else { return nil }
  let rgb = sums.map { min(255, max(0, Int(($0 / weight * 0.52).rounded()))) }
  return "rgb(\(rgb[0]),\(rgb[1]),\(rgb[2]))"
 }
}
