import Foundation
import CoreGraphics
import ImageIO

@main struct ArtworkColorProbe {
 static func main() {
  // CGImage memory rows: top is red, lower band is blue.
  var pixels = [UInt8](repeating: 255, count: 100 * 100 * 4)
  for y in 0..<100 { for x in 0..<100 {
   let i = (y * 100 + x) * 4
   pixels[i] = y < 70 ? 255 : 0
   pixels[i + 1] = 0
   pixels[i + 2] = y < 70 ? 0 : 255
  } }
  let data = Data(pixels)
  let provider = CGDataProvider(data: data as CFData)!
  let image = CGImage(width: 100, height: 100, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 400,
   space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue),
   provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
  let png = NSMutableData()
  let dest = CGImageDestinationCreateWithData(png, "public.png" as CFString, 1, nil)!
  CGImageDestinationAddImage(dest, image, nil); precondition(CGImageDestinationFinalize(dest))
  precondition(ArtworkColor.bottomBand(png as Data) == "rgb(0,0,133)")
  precondition(ArtworkColor.bottomBand(Data([0,1,2])) == nil)
  print("Artwork bottom-band sampling passed; malformed artwork is optional.")
 }
}
