import AVFoundation
import UniformTypeIdentifiers

// AVFoundation seeks within the embedded MP4 using bounded reads of the parent
// file. No clip extraction, disk cache, music seek or audio-session changes.
final class MotionArtworkSource: NSObject, AVAssetResourceLoaderDelegate {
 let asset: AVURLAsset
 private let url: URL, file: FileHandle, range: MotionArtworkRange, scoped: Bool
 private let queue=DispatchQueue(label:"app.sda.motion-artwork.read",qos:.utility)
 private var closed=false
 init?(url: URL) {
  guard url.isFileURL else {return nil}
  let access=url.startAccessingSecurityScopedResource()
  do {
   let handle=try FileHandle(forReadingFrom:url)
   let size=try handle.seekToEnd()
   guard let selected=try MotionArtworkRange.find(in:handle,size:size) else {
    try? handle.close();if access {url.stopAccessingSecurityScopedResource()};return nil
   }
   self.url=url;file=handle;range=selected;scoped=access
   asset=AVURLAsset(url:URL(string:"sda-motion://local/\(UUID().uuidString).mp4")!)
   super.init();asset.resourceLoader.setDelegate(self,queue:queue)
  } catch {if access {url.stopAccessingSecurityScopedResource()};return nil}
 }
 func close() {asset.cancelLoading();asset.resourceLoader.setDelegate(nil,queue:nil);queue.async { [self] in
  guard !closed else{return};closed=true;try? file.close();if scoped {url.stopAccessingSecurityScopedResource()}
 } }
 func resourceLoader(_ loader: AVAssetResourceLoader, shouldWaitForLoadingOfRequestedResource request: AVAssetResourceLoadingRequest) -> Bool {
  do {
   guard !closed else {throw URLError(.cancelled)}
   if let information=request.contentInformationRequest {
    information.contentType=UTType.mpeg4Movie.identifier
    information.contentLength=Int64(range.length);information.isByteRangeAccessSupported=true
   }
   if let data=request.dataRequest {
    let begin=max(data.requestedOffset,data.currentOffset)
    guard data.requestedOffset>=0,begin>=0,UInt64(begin)<=range.length,data.requestedLength>=0 else {throw URLError(.badServerResponse)}
    let requested=UInt64(data.requestedOffset).addingReportingOverflow(UInt64(data.requestedLength))
    guard data.requestedOffset>=0,!requested.overflow else {throw URLError(.badServerResponse)}
    let end=data.requestsAllDataToEndOfResource ? range.length : min(range.length,requested.partialValue)
    var cursor=UInt64(begin)
    while cursor<end,!request.isCancelled {
     let count=Int(min(65536,end-cursor));try file.seek(toOffset:range.offset+cursor)
     guard let bytes=try file.read(upToCount:count),bytes.count==count else {throw URLError(.cannotDecodeContentData)}
     data.respond(with:bytes);cursor+=UInt64(count)
    }
   }
   if !request.isCancelled {request.finishLoading()}
  } catch {if !request.isCancelled {request.finishLoading(with:error)}}
  return true
 }
}

