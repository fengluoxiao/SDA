"""Exercise the production Swift programme-state methods with a counted analyser boundary."""
from pathlib import Path
import subprocess,tempfile
root=Path(__file__).resolve().parents[1]
source=(root/"apps/mobile/modules/sda-core/ios/SdaPlayer.swift").read_text(encoding="utf-8")
state=source[source.index(" private struct StereoSourceIdentity"):source.index(" var uncommittedRetained")]
prepare=source[source.index(" func prepareStereoTrack("):source.index(" private func measureNativeStereoReference(")]
install=source[source.index(" func prepareNativeStereoReference("):source.index(" func requestSeek(")]
header=r'''
import Foundation
enum SdaError:Error{case message(String)}
final class CompressedInput {
 let isAlac=false;let isSdar=false;let isAv3a=true
 init(url:URL,name:String)throws{}
}
final class SystemRoute {
 var installed=0
 func restartAfterMeasurement(_ text:String,decodeReply:(UnsafeMutablePointer<CChar>?)throws->Any)throws{installed+=1}
}
final class Probe {
 let lock=NSRecursiveLock();var generation=1;var hasPlayback=true
 var systemSpatial:SystemRoute?;var trackHash="";var playbackQueue:[[String:Any]]=[]
 var analysed=0;var installed=0;var onMeasure:(()throws->Void)?
 func locked<T>(_ work:()throws->T)rethrows->T{lock.lock();defer{lock.unlock()};return try work()}
 func decodeReply(_ p:UnsafeMutablePointer<CChar>?)throws->Any{[:]}
 func command(_ op:String,_ args:[String:Any])throws->Any{precondition(op=="measured");installed+=1;return [:]}
 private func measureNativeStereoReference(path:String,alac:Bool,av3a:Bool,valid:()->Bool)throws->String?{
  guard valid()else{return nil};analysed+=1;try onMeasure?();guard valid()else{return nil};return "reference"
 }
'''
tests=r'''
let root=FileManager.default.temporaryDirectory.appendingPathComponent("sda-programme-test-"+UUID().uuidString)
try FileManager.default.createDirectory(at:root,withIntermediateDirectories:true);defer{try? FileManager.default.removeItem(at:root)}
let a=root.appendingPathComponent("a.av3a"),b=root.appendingPathComponent("b.av3a")
try Data("a".utf8).write(to:a);try Data("b".utf8).write(to:b)
let ha=String(repeating:"a",count:64),hb=String(repeating:"b",count:64)
let p=Probe();p.playbackQueue=[["hash":ha],["hash":hb]]
try p.prepareStereoTrack(a.absoluteString,"a.av3a",ha);try p.prepareStereoTrack(b.absoluteString,"b.av3a",hb)
for (url,hash) in [(a,ha),(b,hb),(a,ha),(b,hb)]{
 p.generation+=1;p.trackHash=hash
 let installed=try p.prepareNativeStereoReference(path:url.path,alac:false,av3a:true,token:p.generation,hash:hash);precondition(installed)
}
precondition(p.analysed==2 && p.installed==4,"A-B-A-B must not analyse again")
try p.prepareStereoTrack(a.absoluteString,"a.av3a",ha);precondition(p.analysed==2)
p.systemSpatial=SystemRoute();precondition(try p.prepareNativeStereoReference(path:a.path,alac:false,av3a:true,token:p.generation,hash:ha));precondition(p.systemSpatial?.installed==1);p.systemSpatial=nil
p.trackHash=hb;p.playbackQueue=[["hash":hb]];p.retainProgrammeReferences()
precondition(try p.prepareNativeStereoReference(path:a.path,alac:false,av3a:true,token:p.generation,hash:ha));precondition(p.analysed==3)
try Data("changed source".utf8).write(to:a)
precondition(try p.prepareNativeStereoReference(path:a.path,alac:false,av3a:true,token:p.generation,hash:ha));precondition(p.analysed==4)
p.clearPreparedStereoReferences();p.onMeasure={p.generation+=1}
let before=p.installed;let cancelled=try p.prepareNativeStereoReference(path:a.path,alac:false,av3a:true,token:p.generation,hash:ha)
precondition(!cancelled && before==p.installed,"cancelled source must not install old parameters")
p.onMeasure=nil
precondition(try p.prepareNativeStereoReference(path:a.path,alac:false,av3a:true,token:p.generation,hash:ha))
print("PASS Swift programme state: A-B-A-B, repeat prepare, native/system install, queue pruning, changed source, cancellation, recovery")
'''
# Swift autoclosures cannot throw; materialize each throwing predicate first.
import re
serial=0
def predicate(match):
 global serial
 serial+=1
 return "let check%d=try %s;precondition(check%d)"%(serial,match.group(1),serial)
tests=re.sub(r"precondition\(try (p\.prepareNativeStereoReference\([^\n]*?\))\)",predicate,tests)
with tempfile.TemporaryDirectory(prefix="sda-ios-programme-")as temporary:
 p=Path(temporary)/"main.swift";p.write_text(header+state+prepare+install+"}\n"+tests,encoding="utf-8")
 subprocess.run(["xcrun","swift",str(p)],check=True)
