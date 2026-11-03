from pathlib import Path
import subprocess,tempfile
root=Path(__file__).resolve().parents[1]
source=(root/"apps/mobile/modules/sda-core/ios/Mp4ImportSession.swift").read_text(encoding="utf-8")
tests=r"""
enum SdaError:Error {case message(String)}
var nativeSupported=false
func sda_ios_extract_mpegh_mp4(_ source:UnsafePointer<CChar>?,_ output:UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
 if nativeSupported {try! Data([1,2,3,4]).write(to:URL(fileURLWithPath:String(cString:output!)))}
 return strdup(nativeSupported ? "{}" : "null")
}
func decodeNative(_ p:UnsafeMutablePointer<CChar>?) throws -> Any {defer{free(p)};return try JSONSerialization.jsonObject(with:Data(String(cString:p!).utf8),options:.fragmentsAllowed)}
let fm=FileManager.default;let base=fm.temporaryDirectory.appendingPathComponent("sda-import-io-test-"+UUID().uuidString)
try fm.createDirectory(at:base,withIntermediateDirectories:true);defer{try? fm.removeItem(at:base)}
let input=base.appendingPathComponent("source.mp4"),output=base.appendingPathComponent("output.mhas")
let bytes=Data((0..<2*1024*1024+317).map {UInt8($0&255)});try bytes.write(to:input)
let session=try Mp4ImportSession(source:input,output:output)
let fallback=try session.extractNative(decodeNative);assert(fallback == "null")
for at in [0,1048576,73,2097152] {let read=try session.read(offset:Double(at),count:min(1024*1024,bytes.count-at));assert(Data(base64Encoded:read)==bytes.subdata(in:at..<min(bytes.count,at+1024*1024)))}
for at in stride(from:0,to:bytes.count,by:1024*1024){try session.append(bytes.subdata(in:at..<min(bytes.count,at+1024*1024)).base64EncodedString())}
_ = try session.finish();assert(try Data(contentsOf:output)==bytes)
session.discard();assert(!fm.fileExists(atPath:output.path));assert(fm.fileExists(atPath:input.path))
nativeSupported=true
let native=try Mp4ImportSession(source:input,output:output)
_ = try native.extractNative(decodeNative);_ = try native.finish()
let nativeBytes=try Data(contentsOf:output);assert(nativeBytes==Data([1,2,3,4]))
assert(!fm.fileExists(atPath:output.appendingPathExtension("native.mhas").path))
native.discard();assert(fm.fileExists(atPath:input.path))
print("Swift MP4 import: persistent handles, bounded seek/read/write, exact bytes and cleanup passed")
"""
tests=tests.replace("assert(try Data(contentsOf:output)==bytes)","let actual=try Data(contentsOf:output);assert(actual==bytes)")
with tempfile.TemporaryDirectory(prefix="sda-ios-import-io-")as temporary:
 p=Path(temporary)/"main.swift";p.write_text(source+tests,encoding="utf-8");subprocess.run(["xcrun","swift",str(p)],check=True)
