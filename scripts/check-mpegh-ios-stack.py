"""Compile the exact pinned MHAS parser with the Apple device compiler.
No crash reports / personal device identifiers are uploaded. Compare the
upstream stack frame with SDA's narrowly scoped local-variable fix.
"""
import json, pathlib, subprocess, sys

root = pathlib.Path(__file__).resolve().parent.parent
out = pathlib.Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
prepared = out / "mpegh-stack-probe"
subprocess.run(["node", str(root / "scripts/prepare-mpegh.mjs"), str(prepared)], check=True)
sdk = subprocess.check_output(["xcrun", "--sdk", "iphoneos", "--show-sdk-path"], text=True).strip()
clang = subprocess.check_output(["xcrun", "--sdk", "iphoneos", "--find", "clang"], text=True).strip()
report = {}
for label, source in [("upstream", root / "vendor/libmpegh/decoder/impeghd_mhas_parse.c"),
                      ("fixed", prepared / "decoder/impeghd_mhas_parse.c")]:
    obj = out / ("mhas-" + label + ".o")
    subprocess.run([clang, "-target", "arm64-apple-ios16.0", "-isysroot", sdk,
                    "-O2", "-fstack-usage", "-DLC_LEVEL_4", "-std=c99", "-I",
                    str(prepared / "decoder"), "-c", str(source), "-o", str(obj)], check=True)
    rows = obj.with_suffix(".su").read_text().splitlines()
    sizes = [int(row.split("\t")[1]) for row in rows
             if row.split("\t")[0].endswith(":impeghd_mhas_parse")]
    if len(sizes) != 1: raise RuntimeError("Missing MHAS stack usage: " + str(rows))
    report[label + "StackBytes"] = sizes[0]
report["limitBytes"] = 64 * 1024
report["ok"] = report["fixedStackBytes"] <= report["limitBytes"] and report["upstreamStackBytes"] > 544 * 1024
(out / "mpegh-stack.json").write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
if not report["ok"]: raise RuntimeError("MHAS parser regressed to unsafe device stack usage")
