import Foundation

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(s)=self {return s};return nil }
}
@main struct Av3aRouteProbe {
 static func main() throws {
  let fixture=try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))
  precondition(CompressedInput.av3aSpeakerBedHeader(Data(fixture.prefix(9))),"verified 12-bed profile must retain the system route")
  var mixed=Array(fixture.prefix(9));mixed[2]=(mixed[2]&0xf1)|0x02
  precondition(!CompressedInput.av3aSpeakerBedHeader(Data(mixed)),"Mix objects must use the native scene path")
  var hoa=mixed;hoa[2]=(hoa[2]&0xf1)|0x04
  precondition(!CompressedInput.av3aSpeakerBedHeader(Data(hoa)))
  precondition(!CompressedInput.av3aSpeakerBedHeader(Data(fixture.prefix(3))))
  var stereo=Array(fixture.prefix(9));stereo[4] &= 0xe0;stereo[5]=(stereo[5]&0x3f)|0x40
  precondition(!CompressedInput.av3aSpeakerBedHeader(Data(stereo)))
  // Exercise raw-file capability probing and verify it leaves the read cursor at zero.
  let folder=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
  defer { try? FileManager.default.removeItem(at:folder) }
  for (name, bytes, expected) in [("bed.av3a",fixture,true),("mixed.av3a",Data(mixed),false)] {
   let url=folder.appendingPathComponent(name)
   try bytes.write(to:url)
   let input=try CompressedInput(url:url,name:name)
   precondition(input.isAv3a && input.av3aSpeakerBed == expected)
   let packet=try input.next(chunkBytes:9)
   precondition(packet == Data(bytes.prefix(9)),"capability probe consumed the first access-unit header")
  }
  print("AV3A route capability: verified 7.1.4 bed retained; Mix/HOA/stereo/truncated headers bypass fixed 12-speaker output.")
 }
}
