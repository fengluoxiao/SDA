"""Generate reference energies from the exact native KU100 speaker filters."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess

spec = importlib.util.spec_from_file_location("simulator", Path(__file__).with_name("room-simulator.py"))
sim = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sim)
np = sim.np

def levels(speakers, root):
    manifest = json.loads((root / "hrtf-set.json").read_text(encoding="utf-8"))
    def vector(az, el):
        a, e = np.deg2rad([az, el])
        return np.array([np.cos(a)*np.cos(e), np.sin(a)*np.cos(e), np.sin(e)])
    pairs = {stage: [] for stage in ["direct", "early", "full"]}
    for speaker in speakers:
        target = vector(speaker["azimuth"], speaker["elevation"])
        # Rust max_by keeps the last equal entry.
        entry = max(reversed(manifest["positions"]), key=lambda p: np.dot(target, vector(p["azimuth"], p["elevation"])))
        dry = np.fromfile(root / entry["dry"], dtype="<f4").reshape(2, -1)
        wet = np.fromfile(root / entry["wet"], dtype="<f4").reshape(2, -1)
        dry = np.pad(dry, ((0, 0), (0, wet.shape[1]-dry.shape[1])))
        late = np.clip((np.arange(wet.shape[1])-128-50*48+240)/480, 0, 1)
        pairs["direct"].append(dry)
        pairs["early"].append(dry + .04*(wet-dry)*(1-late))
        pairs["full"].append(dry + .04*(wet-dry))
    return {stage: sim.pink_reference(value) for stage, value in pairs.items()}

if __name__ == "__main__":
    script = "const {validateConfig}=require('./apps/desktop/room-lab.cjs');console.log(JSON.stringify(['2.0','2.1','5.1','5.1.2','5.1.4','7.1.2','7.1.4','9.1.2','9.1.4','9.1.6'].map(layout=>validateConfig({layout,length:6,width:4,height:2.8,earHeight:1.2,placement:.85,material:'treated',order:6}))));"
    configs = json.loads(subprocess.run([os.environ.get("NODE_BINARY","node"), "-e", script], capture_output=True, text=True, check=True).stdout)
    result = {"sampleRate":48000,"wetWeight":.04,"onsetSample":128,"earlyMs":50,
              "metric":"20 Hz-20 kHz pink-noise response energy; equal independent speaker feeds",
              "layouts":{c["layout"]:{mode:levels(c["speakers"],Path("apps/web/public")/directory)
                         for mode,directory in [("raw","hrtf-raw"),("calibrated","hrtf")]}
                         for c in configs}}
    Path("apps/web/src/ku100-comparison-levels.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(result["layouts"]["7.1.4"],indent=2))
