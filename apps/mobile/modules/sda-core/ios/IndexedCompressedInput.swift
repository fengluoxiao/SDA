import Foundation
import CoreMedia
import CryptoKit

// A clear EC-3 continuation after AVFoundation loses its sample cursor.
// Never reset the decoder or guess EOF: prove the submitted prefix before
// returning any new bytes. Only metadata and one packet are retained.
final class IndexedCompressedInput {
 private struct Box { let kind: UInt32; let body: Range<Int> }
 private struct Packet { let offset: UInt64; let size: Int; let time: Int64; let duration: Int64 }
 private struct Index {
  let packets: [Packet]; let timescale: Int32
  let editStart: Int64; let leading: Int64; let endTime: Int64
 }
 private let file: FileHandle
 private let index: Index
 private var position = 0
 private var end = 0
 private var pending = Data()
 private var pendingOffset = 0
 private(set) var verifiedPrefixBytes: UInt64 = 0
 private(set) var recoveryStartSample = 0
 var sampleCount: Int { index.packets.count }
 var samplePosition: Int { position }
 private init(file: FileHandle, index: Index) {
  self.file=file; self.index=index
  end=index.packets.firstIndex(where: { $0.time >= index.endTime }) ?? index.packets.count
 }
 deinit { try? file.close() }
 private static func invalid(_ reason: String) -> SdaError { .message("MP4 压缩包索引无效: " + reason) }
 private static func tag(_ s: String) -> UInt32 { s.utf8.reduce(0) { ($0 << 8) | UInt32($1) } }
 private static func number(_ d: Data, _ at: Int, _ count: Int) throws -> UInt64 {
  guard at >= 0, count <= d.count, at <= d.count-count else { throw invalid("元数据已截断") }
  return d[at..<at+count].reduce(0) { ($0 << 8) | UInt64($1) }
 }
 private static func children(_ d: Data, _ range: Range<Int>) throws -> [Box] {
  var out:[Box]=[]; var at=range.lowerBound
  while at < range.upperBound {
   guard range.upperBound-at >= 8, out.count < 100_000 else { throw invalid("box 边界") }
   let kind=UInt32(try number(d,at+4,4)); var size=try number(d,at,4); var header=8
   if size == 1 { guard range.upperBound-at >= 16 else { throw invalid("64 位 box 头") }; size=try number(d,at+8,8); header=16 }
   if size == 0 { size=UInt64(range.upperBound-at) }
   guard size >= UInt64(header), size <= UInt64(range.upperBound-at) else { throw invalid("box 长度") }
   let stop=at+Int(size); out.append(Box(kind:kind,body:at+header..<stop)); at=stop
  }
  return out
 }
 private static func child(_ d: Data, _ range: Range<Int>, _ kind: String) throws -> Range<Int>? {
  try children(d,range).first(where: { $0.kind == tag(kind) })?.body
 }
 private static func required(_ d: Data, _ range: Range<Int>, _ kind: String) throws -> Range<Int> {
  guard let value=try child(d,range,kind) else { throw invalid("缺少 " + kind) }; return value
 }
 private static func exact(_ file: FileHandle, offset: UInt64, count: Int) throws -> Data {
  guard count >= 0, count <= 64*1024*1024 else { throw invalid("读取长度") }
  try file.seek(toOffset:offset); var data=Data(); data.reserveCapacity(count)
  while data.count < count {
   guard let part=try file.read(upToCount:min(65536,count-data.count)), !part.isEmpty else { throw invalid("媒体数据已截断") }
   data.append(part)
  }
  return data
 }
 private static func ticks(_ value: UInt64, from: Int32, to: Int32) throws -> Int64 {
  guard value <= UInt64(Int64.max), from > 0, to > 0 else { throw invalid("时间范围") }
  let t=CMTimeConvertScale(CMTime(value:Int64(value),timescale:from),timescale:to,method:.roundTowardZero)
  guard t.isNumeric, t.value >= 0 else { throw invalid("时间转换") }; return t.value
 }
 private static func scale(_ d: Data, _ range: Range<Int>) throws -> Int32 {
  let version=try number(d,range.lowerBound,1); guard version <= 1 else { throw invalid("时间头版本") }
  let offset=version == 1 ? 20 : 12
  guard range.count >= offset+4 else { throw invalid("时间头长度") }
  let v=try number(d,range.lowerBound+offset,4)
  guard v > 0, v <= UInt64(Int32.max) else { throw invalid("timescale") }; return Int32(v)
 }
 static func open(_ url: URL, trackID: CMPersistentTrackID? = nil) throws -> IndexedCompressedInput? {
  if let trackID, trackID <= 0 { return nil }
  let f=try FileHandle(forReadingFrom:url)
  do {
   let length=try f.seekToEnd(); var at:UInt64=0; var moov:Data?; var mdats:[Range<UInt64>]=[]
   while at < length {
    guard length-at >= 8 else { throw invalid("文件尾 box") }
    let h=try exact(f,offset:at,count:Int(min(16,length-at))); var size=try number(h,0,4); let kind=UInt32(try number(h,4,4)); var header:UInt64=8
    if size == 1 { guard h.count >= 16 else { throw invalid("文件头") }; size=try number(h,8,8); header=16 }
    if size == 0 { size=length-at }
    guard size >= header, size <= length-at else { throw invalid("文件 box 范围") }
    if kind == tag("moov") {
     guard moov == nil, size-header <= 64*1024*1024 else { throw invalid("moov 长度或重复") }
     moov=try exact(f,offset:at+header,count:Int(size-header))
    } else if kind == tag("mdat") { mdats.append(at+header..<at+size) }
    at += size
   }
   guard let data=moov, let parsed=try parse(data,mdats:mdats,trackID:trackID) else { try f.close(); return nil }
   return IndexedCompressedInput(file:f,index:parsed)
  } catch { try? f.close(); throw error }
 }
 private static func parse(_ d: Data, mdats: [Range<UInt64>], trackID: CMPersistentTrackID?) throws -> Index? {
  let whole=0..<d.count
  if try child(d,whole,"mvex") != nil { return nil }
  let movieScale: Int32? = try child(d,whole,"mvhd").map { try scale(d,$0) }
  for track in try children(d,whole) where track.kind == tag("trak") {
   guard let mdia=try child(d,track.body,"mdia"), let minf=try child(d,mdia,"minf"), let stbl=try child(d,minf,"stbl"), let stsd=try child(d,stbl,"stsd") else { continue }
   guard let handler=try child(d,mdia,"hdlr"), handler.count >= 12, try number(d,handler.lowerBound+8,4) == UInt64(tag("soun")) else { continue }
   if let trackID {
    guard let header=try child(d,track.body,"tkhd"), header.count >= 16 else { continue }
    let version=try number(d,header.lowerBound,1);guard version <= 1 else { continue }
    let offset=version == 1 ? 20 : 12;guard header.count >= offset+4 else { throw invalid("track ID 长度") }
    if try number(d,header.lowerBound+offset,4) != UInt64(trackID) { continue }
   }
   guard stsd.count >= 8, try number(d,stsd.lowerBound+4,4) == 1 else { continue }
   let entries=try children(d,stsd.lowerBound+8..<stsd.upperBound)
   guard entries.count == 1, entries[0].kind == tag("ec-3") else { continue }
   let entry=entries[0].body
   guard entry.count >= 28, try number(d,entry.lowerBound+6,2) == 1, try number(d,entry.lowerBound+8,2) == 0 else { continue }
   if try child(d,entry.lowerBound+28..<entry.upperBound,"sinf") != nil { continue }
   // External references and reordered audio keep the existing reader path.
   guard let dinf=try child(d,minf,"dinf"), let dref=try child(d,dinf,"dref"), dref.count >= 8,
    try number(d,dref.lowerBound+4,4) == 1 else { return nil }
   let references=try children(d,dref.lowerBound+8..<dref.upperBound)
   guard references.count == 1, references[0].kind == tag("url "),references[0].body.count >= 4,
    try number(d,references[0].body.lowerBound,4) & 1 == 1 else { return nil }
   if try child(d,stbl,"ctts") != nil { return nil }
   let timescale=try scale(d,required(d,mdia,"mdhd"))
   guard let sz=try child(d,stbl,"stsz"), let sc=try child(d,stbl,"stsc"), let ts=try child(d,stbl,"stts") else { return nil }
   guard sz.count >= 12 else { throw invalid("stsz 长度") }
   let total=Int(try number(d,sz.lowerBound+8,4)); guard total > 0, total <= 4_194_304 else { throw invalid("样本数量") }
   let fixed=Int(try number(d,sz.lowerBound+4,4)); guard fixed != 0 || sz.count >= 12+total*4 else { throw invalid("stsz 样本表已截断") }
   var sizes=[Int](); sizes.reserveCapacity(total)
   for i in 0..<total { let size=fixed == 0 ? Int(try number(d,sz.lowerBound+12+i*4,4)) : fixed; guard size <= 1024*1024 else { throw invalid("单包过大") }; sizes.append(size) }
   guard ts.count >= 8 else { throw invalid("stts 长度") }
   let timingCount=Int(try number(d,ts.lowerBound+4,4)); guard timingCount > 0, timingCount <= 4_194_304, ts.count >= 8+timingCount*8 else { throw invalid("stts 数量或长度") }
   var durations=[Int64](); durations.reserveCapacity(total)
   for i in 0..<timingCount {
    let n=Int(try number(d,ts.lowerBound+8+i*8,4)); let duration=Int64(try number(d,ts.lowerBound+12+i*8,4)); guard duration > 0 else { throw invalid("样本时长") }
    if durations.count < total { durations.append(contentsOf:repeatElement(duration,count:min(n,total-durations.count))) }
   }
   guard durations.count == total else { throw invalid("stts 少于样本数量") }
   let offsetsBox:Range<Int>; let width:Int
   if let v=try child(d,stbl,"stco") { offsetsBox=v; width=4 } else if let v=try child(d,stbl,"co64") { offsetsBox=v; width=8 } else { throw invalid("缺少 chunk 偏移") }
   guard offsetsBox.count >= 8 else { throw invalid("chunk 表长度") }
   let chunks=Int(try number(d,offsetsBox.lowerBound+4,4)); guard chunks > 0, chunks <= 4_194_304, offsetsBox.count >= 8+chunks*width else { throw invalid("chunk 数量或长度") }
   guard sc.count >= 8 else { throw invalid("stsc 长度") }
   let runCount=Int(try number(d,sc.lowerBound+4,4)); guard runCount > 0, runCount <= chunks, sc.count >= 8+runCount*12 else { throw invalid("stsc 数量或长度") }
   var runs:[(first:Int,count:Int)]=[]
   for i in 0..<runCount {
    let first=Int(try number(d,sc.lowerBound+8+i*12,4)); let count=Int(try number(d,sc.lowerBound+12+i*12,4)); let description=try number(d,sc.lowerBound+16+i*12,4)
    guard description == 1 else { return nil }
    guard first >= 1, first <= chunks, count > 0, runs.last.map({ first > $0.first }) ?? (first == 1) else { throw invalid("stsc 范围") }
    runs.append((first,count))
   }
   var packets=[Packet](); packets.reserveCapacity(total); var run=0; var time:Int64=0
   for chunk in 0..<chunks {
    while run+1 < runs.count && runs[run+1].first <= chunk+1 { run += 1 }
    var offset=try number(d,offsetsBox.lowerBound+8+chunk*width,width)
    for _ in 0..<min(runs[run].count,total-packets.count) {
     let i=packets.count; let (stop,overflow)=offset.addingReportingOverflow(UInt64(sizes[i]))
     guard !overflow, mdats.contains(where: { offset >= $0.lowerBound && stop <= $0.upperBound }) else { throw invalid("样本超出 mdat") }
     packets.append(Packet(offset:offset,size:sizes[i],time:time,duration:durations[i])); time += durations[i]; offset=stop
    }
    if packets.count == total { break }
   }
   guard packets.count == total else { throw invalid("chunk 少于样本数量") }
   var editStart:Int64=0; var leading:Int64=0; var endTime=time
   if let edts=try child(d,track.body,"edts"), let elst=try child(d,edts,"elst") {
    guard let movieScale, elst.count >= 8 else { return nil }
    let version=try number(d,elst.lowerBound,1); guard version <= 1 else { return nil }
    let count=Int(try number(d,elst.lowerBound+4,4)); let stride=version == 1 ? 20 : 12
    guard count <= 100_000, elst.count >= 8+count*stride else { throw invalid("edit 数量或长度") }
    var found=false
    for i in 0..<count {
     let pos=elst.lowerBound+8+i*stride; let duration=try number(d,pos,version == 1 ? 8 : 4)
     let raw=try number(d,pos+(version == 1 ? 8 : 4),version == 1 ? 8 : 4)
     let media=version == 1 ? Int64(bitPattern:raw) : Int64(Int32(bitPattern:UInt32(raw)))
     guard try number(d,pos+(version == 1 ? 16 : 8),4) == 0x10000 else { return nil }
     let length=try ticks(duration,from:movieScale,to:timescale)
     if media == -1 {
      if !found { let (v,o)=leading.addingReportingOverflow(length); guard !o else { throw invalid("edit 溢出") }; leading=v }
      continue
     }
     guard media >= 0, !found else { return nil }; found=true; editStart=media
     let (declaredEnd,overflow)=media.addingReportingOverflow(length); guard !overflow else { throw invalid("edit 终点溢出") }
     // A declared edit cannot create samples beyond the physical stts/stsz tail.
     endTime=min(time,declaredEnd)
    }
    if count > 0 && !found { return nil }
   }
   return Index(packets:packets,timescale:timescale,editStart:editStart,leading:leading,endTime:endTime)
  }
  return nil
 }
 private func mediaTicks(_ t: CMTime) throws -> Int64 {
  let v=CMTimeConvertScale(t,timescale:index.timescale,method:.roundTowardZero)
  guard v.isNumeric else { throw Self.invalid("恢复时间戳") }
  let (a,o)=v.value.addingReportingOverflow(index.editStart); let (b,p)=a.subtractingReportingOverflow(index.leading)
  guard !o, !p else { throw Self.invalid("恢复时间戳溢出") }; return b
 }
 private func packet(at time: Int64) -> Int {
  var lo=0; var hi=index.packets.count
  while lo < hi { let mid=(lo+hi)/2; if index.packets[mid].time <= time { lo=mid+1 } else { hi=mid } }
  return max(0,lo-1)
 }
 private func endpoint(start:Int, bytes:UInt64) -> (position:Int,offset:Int,endTime:Int64?)? {
  var i=start; var left=bytes
  while left > 0 && i < end {
   let size=UInt64(index.packets[i].size)
   if left < size { return (i,Int(left),nil) }
   left -= size; i += 1
  }
  guard left == 0 else { return nil }
  return (i,0,i > start ? index.packets[i-1].time+index.packets[i-1].duration : nil)
 }
 private func seek(start:Int,bytes:UInt64) throws {
  guard let p=endpoint(start:start,bytes:bytes) else { throw Self.invalid("已提交字节超出轨道") }
  position=p.position; pending=Data(); pendingOffset=0
  if p.offset > 0 {
   let sample=index.packets[position]; pending=try Self.exact(file,offset:sample.offset,count:sample.size);pendingOffset=p.offset;position += 1
  }
 }
 func verifyContinuation(firstPTS:CMTime,lastEnd:CMTime?,submittedBytes:UInt64,digest:Data) throws {
  let first=try mediaTicks(firstPTS); guard first >= 0 else { throw Self.invalid("恢复起点") }
  // AVAssetReader may return codec preroll. The raw start and timestamp-mapped
  // start are the only candidates; hashes/timestamps must resolve them uniquely.
  let predicted=packet(at:first); let candidates=Array(Set([0,predicted])).sorted()
  let expectedEnd=try lastEnd.map { try mediaTicks($0) }; var matches=[Int]()
  for start in candidates {
   guard let endpoint=endpoint(start:start,bytes:submittedBytes) else { continue }
   if let expectedEnd, let actualEnd=endpoint.endTime {
    let (difference,overflow)=actualEnd.subtractingReportingOverflow(expectedEnd)
    if overflow || difference < -1 || difference > 1 { continue }
   }
   try seek(start:start,bytes:0); var hash=SHA256(); var left=submittedBytes
   while left > 0 {
    guard let data=try next(chunkBytes:Int(min(left,65536))), !data.isEmpty else { throw Self.invalid("恢复前缀已截断") }
    hash.update(data:data);left -= UInt64(data.count)
   }
   if Data(hash.finalize()) == digest { matches.append(start) }
  }
  guard matches.count == 1 else { throw Self.invalid("恢复前缀或时间戳无法唯一验证，未跳过音频") }
  try seek(start:matches[0],bytes:submittedBytes);verifiedPrefixBytes=submittedBytes;recoveryStartSample=matches[0]
 }
 func next(chunkBytes:Int) throws -> Data? {
  let limit=max(1,min(chunkBytes,1024*1024));var result=Data();result.reserveCapacity(limit)
  while result.count < limit {
   if pendingOffset == pending.count {
    guard position < end else { break };let sample=index.packets[position]
    pending=try Self.exact(file,offset:sample.offset,count:sample.size);pendingOffset=0;position += 1
    if pending.isEmpty { continue }
   }
   let take=min(limit-result.count,pending.count-pendingOffset)
   result.append(pending.subdata(in:pendingOffset..<pendingOffset+take));pendingOffset += take
  }
  return result.isEmpty ? nil : result
 }
}
