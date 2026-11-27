import Foundation
import CoreGraphics
import ImageIO

func sample(_ path:String,_ rect:CGRect)throws->[UInt8]{
 guard let source=CGImageSourceCreateWithURL(URL(fileURLWithPath:path) as CFURL,nil),let image=CGImageSourceCreateImageAtIndex(source,0,nil),
  let crop=image.cropping(to:CGRect(x:rect.minX*CGFloat(image.width),y:rect.minY*CGFloat(image.height),width:rect.width*CGFloat(image.width),height:rect.height*CGFloat(image.height))) else{throw NSError(domain:"Missing screenshot region",code:1)}
 var data=[UInt8](repeating:0,count:64*64*4)
 try data.withUnsafeMutableBytes{storage in
  guard let context=CGContext(data:storage.baseAddress,width:64,height:64,bitsPerComponent:8,bytesPerRow:64*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else{throw NSError(domain:"Screen sample context failed",code:1)}
  context.draw(crop,in:CGRect(x:0,y:0,width:64,height:64))
 }
 return data
}
let args=CommandLine.arguments
guard args.count==7 else{fatalError("Pass before/after PNG and normalized rectangle")}
let rect=CGRect(x:Double(args[3])!,y:Double(args[4])!,width:Double(args[5])!,height:Double(args[6])!)
let before=try sample(args[1],rect),after=try sample(args[2],rect)
var total=0
for pixel in 0..<(64*64){for channel in 0..<3{total+=abs(Int(before[pixel*4+channel])-Int(after[pixel*4+channel]))}}
print(Double(total)/Double(64*64*3))
