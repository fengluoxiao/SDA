#!/usr/bin/env python3
"""Stage pinned public LiteRT/QNN/NeuroPilot runtimes as private APK assets.

Only selected QNN ZIP members are fetched with HTTP Range (the SDK is >2 GiB).
No proprietary compiler tools, development headers or sample models are shipped.
"""
import argparse, hashlib, io, json, os, shutil, subprocess, sys, tarfile, tempfile, urllib.request, zipfile
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
VERSION = "2.3.0"
MAVEN = "https://dl.google.com/dl/android/maven2/com/google/ai/edge/litert"
QNN = "https://softwarecenter.qualcomm.com/api/download/software/sdks/Qualcomm_AI_Runtime_Community/All/2.50.0.260828/v2.50.0.260828.zip"
MTK = "https://s3.ap-southeast-1.amazonaws.com/mediatek.neuropilot.com/66f2c33a-2005-4f0b-afef-2053c8654e4f.gz"
QNN_ROOT = "qairt/2.50.0.260828/"
LOCK = ROOT / "scripts/android-npu-runtime-lock.json"
def digest(b): return hashlib.sha256(b).hexdigest()
def get(url, headers=None):
    # Windows' system TLS handles these official vendor download hosts more
    # reliably than the bundled Python OpenSSL transport. Keep the same HTTPS,
    # response-size and exact Range validation with either transport.
    curl = shutil.which("curl.exe") if sys.platform == "win32" else None
    if curl:
        with tempfile.TemporaryDirectory(prefix="sda-npu-download-") as temp:
            body = Path(temp)/"body"; head = Path(temp)/"headers"
            command = [curl, "--fail", "--location", "--silent", "--show-error",
                "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5",
                "--connect-timeout", "20", "--max-time", "180", "--retry", "2",
                "--max-filesize", str(256*1024*1024), "--dump-header", str(head), "--output", str(body)]
            for name,value in (headers or {}).items(): command += ["--header", f"{name}: {value}"]
            command.append(url)
            result = subprocess.run(command, capture_output=True, timeout=600)
            if result.returncode: raise RuntimeError(f"Download failed: {url}: {result.stderr.decode(errors='replace')[-1000:]}")
            response_headers={};status=0
            for line in head.read_text(encoding="iso-8859-1").splitlines():
                if line.startswith("HTTP/"):
                    status=int(line.split()[1]);response_headers={}
                elif ":" in line:
                    name,value=line.split(":",1);response_headers[name.lower()]=value.strip()
            if headers and "Range" in headers and status!=206:
                raise RuntimeError("Range request was ignored; refusing full SDK download")
            if body.stat().st_size>256*1024*1024:raise RuntimeError("Download exceeded bounded response limit")
            data=body.read_bytes();length=int(response_headers.get("content-length","0"))
            if length and len(data)!=length:raise RuntimeError("Incomplete download response")
            return data,response_headers,status
    last = None
    for _ in range(5):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers or {}), timeout=60) as r:
                response_headers={k.lower():v for k,v in r.headers.items()}
                if headers and "Range" in headers and r.status!=206:
                    raise RuntimeError("Range request was ignored; refusing full SDK download")
                length=int(response_headers.get("content-length","0"))
                if length>256*1024*1024:raise RuntimeError("Unexpected oversized download")
                data=r.read(256*1024*1024+1)
                if len(data)>256*1024*1024:raise RuntimeError("Download exceeded bounded response limit")
                if length and len(data)!=length:raise RuntimeError("Incomplete download response")
                return data,response_headers,r.status
        except Exception as e: last=e
    raise RuntimeError(f"Download failed: {url}: {last}")
class RangeZip(io.RawIOBase):
    def __init__(self,url):
        self.url=url;self.pos=0
        tail,h,status=get(url,{"Range":"bytes=-65536"})
        if status!=206 or not h.get("content-range"):raise RuntimeError("QNN download requires HTTP Range; refusing full SDK download")
        self.size=int(h["content-range"].split("/")[-1]);self.cache_start=self.size-len(tail);self.cache=tail
    def seekable(self):return True
    def tell(self):return self.pos
    def seek(self,offset,whence=0):
        self.pos=offset if whence==0 else self.pos+offset if whence==1 else self.size+offset
        if self.pos<0:raise ValueError("negative seek")
        return self.pos
    def read(self,n=-1):
        n=min(self.size-self.pos,n if n>=0 else self.size-self.pos)
        if n<=0:return b""
        result=[]
        while n:
            if not(self.cache_start<=self.pos<self.cache_start+len(self.cache)):
                end=min(self.size,self.pos+4*1024*1024)-1
                self.cache,h,status=get(self.url,{"Range":f"bytes={self.pos}-{end}"})
                if status!=206 or h.get("content-range","").split("/")[0]!=f"bytes {self.pos}-{end}":raise RuntimeError("Invalid QNN range response")
                self.cache_start=self.pos
            start=self.pos-self.cache_start;count=min(n,len(self.cache)-start)
            result.append(self.cache[start:start+count]);self.pos+=count;n-=count
        return b"".join(result)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--cache",type=Path,default=Path(os.environ.get("SDA_NPU_DOWNLOAD_CACHE",str(Path.home()/".cache/sda-npu")))/VERSION)
    parser.add_argument("--record-lock",action="store_true")
    parser.add_argument("--accept-qualcomm-license",action="store_true",help="Confirm you have reviewed and accepted the Qualcomm runtime terms")
    parser.add_argument("--accept-qairt-license",action="store_true",help="Confirm separate Qualcomm AI Stack runtime license acceptance")
    parser.add_argument("--legacy-only",action="store_true",help="Stage legacy Hexagon NN; omit modern QNN/NeuroPilot")
    args=parser.parse_args()
    if not args.accept_qualcomm_license and os.environ.get("SDA_ACCEPT_QUALCOMM_RUNTIME_LICENSE")!="1":
        parser.error("Qualcomm runtime terms require acceptance. Review docs/licenses/qualcomm-hexagon-runtime.txt; then explicitly set SDA_ACCEPT_QUALCOMM_RUNTIME_LICENSE=1 or use --accept-qualcomm-license.")
    if not args.legacy_only and not args.accept_qairt_license and os.environ.get("SDA_ACCEPT_QAIRT_RUNTIME_LICENSE")!="1":
        parser.error("Modern QAIRT has a separate AI Stack license. Review that agreement and explicitly confirm acceptance; --legacy-only uses the already-approved old Hexagon runtime.")
    args.cache.mkdir(parents=True,exist_ok=True)
    out=args.output/"sda-npu";out.mkdir(parents=True,exist_ok=True)
    locked={} if args.record_lock else json.loads(LOCK.read_text())["files"]
    entries={}; notices=[]
    def save(name,data,source):
        h=digest(data)
        if not args.record_lock and (name not in locked or locked[name]["sha256"]!=h):raise RuntimeError(f"Runtime integrity mismatch: {name}")
        # These are extracted assets, not directly mapped JNI libraries. A .so
        # suffix forces uncompressed APK storage; .bin permits AAPT deflation.
        asset=name+".bin"
        dst=out/asset;dst.parent.mkdir(parents=True,exist_ok=True)
        if not dst.exists() or digest(dst.read_bytes())!=h:
            tmp=dst.with_suffix(dst.suffix+".part");tmp.write_bytes(data);tmp.replace(dst)
        print(f"Staged {name} ({len(data)/1048576:.2f} MiB)",flush=True)
        entries[name]={"sha256":h,"size":len(data),"source":source,"asset":asset}
    def cache_blob(name,url):
        path=args.cache/name
        if not path.exists():
            data,_,_=get(url);tmp=path.with_suffix(path.suffix+".part");tmp.write_bytes(data);tmp.replace(path)
        return path
    headers_url="https://storage.googleapis.com/mirror.tensorflow.org/storage.cloud.google.com/download.tensorflow.org/tflite/hexagon_nn_headers_v1.20.0.9.tgz"
    headers=cache_blob("hexagon-headers.tgz",headers_url)
    if digest(headers.read_bytes())!="f577b4c150b72e11e9dfb3f9d14f9772ba8fe460f7d65c84a7327ea9bef44d8e":raise RuntimeError("Legacy Hexagon headers/interface checksum mismatch")
    with tarfile.open(headers,"r:gz") as archive:
        save("legacy/libhexagon_interface.so",archive.extractfile("hexagon/arm64-v8a/libhexagon_interface.so").read(),headers_url)
        notices.append(archive.extractfile("hexagon/LICENSE").read().decode())
    skel_url="https://storage.googleapis.com/download.tensorflow.org/tflite/hexagon_nn_skel_v1.20.0.1.run"
    skel=cache_blob("hexagon-skel.run",skel_url).read_bytes()
    # Read the archive as data. Never execute a downloaded self-extractor.
    offset=skel.find(b"\x1f\x8b\x08")
    if offset<0:raise RuntimeError("Legacy Hexagon archive layout mismatch")
    with tarfile.open(fileobj=io.BytesIO(skel[offset:]),mode="r:gz") as archive:
        for name in ["libhexagon_nn_skel.so","libhexagon_nn_skel_v65.so","libhexagon_nn_skel_v66.so"]:
            save("legacy/"+name,archive.extractfile("./"+name).read(),skel_url)
        notices.append(archive.extractfile("./CLICK_THROUGH_LICENSE.txt").read().decode())
    for artifact in ([] if args.legacy_only else ["litert","litert-npu-runtime-qualcomm","litert-npu-runtime-mediatek"]):
        url=f"{MAVEN}/{artifact}/{VERSION}/{artifact}-{VERSION}.aar"
        path=cache_blob(artifact+".aar",url)
        with zipfile.ZipFile(path) as archive:
            for name in archive.namelist():
                if name.startswith("jni/arm64-v8a/") and name.endswith(".so"):
                    save("common/"+Path(name).name,archive.read(name),url)
                if name in ["LICENSE","THIRD_PARTY_NOTICE.txt"]:
                    notices.append(f"===== {artifact}: {name} =====\n"+archive.read(name).decode(errors="replace"))
    if not args.legacy_only:
        qnn_files={"lib/aarch64-android/"+n for n in ["libQnnSystem.so","libQnnIr.so","libQnnSaver.so","libQnnHtp.so","libQnnHtpPrepare.so"]}
        for version in [68,69,73,75,79,81]:
            qnn_files.add(f"lib/aarch64-android/libQnnHtpV{version}Stub.so")
            qnn_files.add(f"lib/hexagon-v{version}/unsigned/libQnnHtpV{version}Skel.so")
        # Cache each extracted member, never the multi-gigabyte SDK archive.
        missing=[name for name in sorted(qnn_files) if not (args.cache/Path(name).name).exists()]
        if missing:
            with zipfile.ZipFile(RangeZip(QNN)) as archive:
                for name in missing:
                    data=archive.read(QNN_ROOT+name);(args.cache/Path(name).name).write_bytes(data)
                for info in archive.infolist():
                    if info.file_size<1024*1024 and ("LICENSE" in info.filename.upper() or "NOTICE" in info.filename.upper()) and not info.is_dir():
                        try:notices.append(f"===== QAIRT: {info.filename} =====\n"+archive.read(info).decode())
                        except UnicodeError:pass
            (args.cache/"qnn-notices.txt").write_text("\n".join(notices),encoding="utf-8")
        for name in sorted(qnn_files):save("common/"+Path(name).name,(args.cache/Path(name).name).read_bytes(),QNN)
        if (args.cache/"qnn-notices.txt").exists():notices.append((args.cache/"qnn-notices.txt").read_text(encoding="utf-8"))
        mtk=cache_blob("neuropilot.gz",MTK)
        with tarfile.open(mtk,"r:gz") as archive:
            wanted={"neuro_pilot/v8_0_10/usdk/lib64/libneuronusdk_adapter.mtk.so","neuro_pilot/v9_0_3/usdk/lib64/libneuronusdk_adapter.so"}
            found=set()
            for info in archive:
                name=info.name.removeprefix("./")
                if name in wanted:
                    f=archive.extractfile(info);save("common/"+Path(name).name,f.read(),MTK);found.add(name)
                if info.isfile() and info.size<1024*1024 and ("LICENSE" in name.upper() or "NOTICE" in name.upper()):
                    f=archive.extractfile(info)
                    try:notices.append(f"===== NeuroPilot: {name} =====\n"+f.read().decode())
                    except UnicodeError:pass
            if found!=wanted:raise RuntimeError("NeuroPilot SDK layout mismatch")
    manifest={"version":VERSION,"minimumSdk":26,"modernMinimumSdk":31,"abi":"arm64-v8a","assetFormat":"aapt-deflated-bin-v1","files":entries}
    (out/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n",encoding="utf-8")
    (out/"THIRD_PARTY_NOTICES.txt").write_text("\n".join(notices),encoding="utf-8")
    # Only generated runtime files in this known output slot are cleaned.
    active_assets={entry["asset"] for entry in entries.values()}
    for kind in ["common","legacy"]:
        for file in (out/kind).glob("lib*.so*"):
            if file.is_file() and (file.name.endswith(".so") or file.name.endswith(".so.bin")):
                if kind+"/"+file.name not in active_assets:file.unlink()
    if args.record_lock:LOCK.write_text(json.dumps(manifest,indent=2)+"\n",encoding="utf-8")
    print(f"Staged {len(entries)} pinned Android NPU libraries ({sum(x['size'] for x in entries.values())/1048576:.1f} MiB)")
if __name__=="__main__":main()
