"""Generate room reflections from the native-prepared shared HRTFHQ receiver."""
import hashlib
import json
import math
from pathlib import Path
import subprocess
import tempfile

import numpy as np
from pyroomacoustics.doa import GridSphere

def load_receiver(manifest_path):
    path = Path(manifest_path)
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if (manifest.get("roomReceiverSchema") != 1 or manifest.get("profileId") != "apple-hrtfhq-542-v1"
            or manifest.get("preparation") != "native-shared-clarity-v1" or manifest.get("sampleRate") != 48000):
        raise ValueError("Receiver must be exported by the native HRTFHQ loader")
    if len(manifest.get("positions", [])) != 542:
        raise ValueError("Incomplete HRTFHQ room receiver")
    directions, responses = [], []
    digest = hashlib.sha256()
    for entry in manifest["positions"]:
        file = (path.parent / entry["dry"]).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError("Receiver file outside its bank")
        data = file.read_bytes()
        digest.update(data)
        pair = np.frombuffer(data, dtype="<f4").reshape(2, -1)
        if pair.shape != (2, 640) or not np.isfinite(pair).all() or not np.any(pair):
            raise ValueError("Invalid prepared HRTFHQ response")
        directions.append([math.radians(entry["azimuth"]), math.pi/2-math.radians(entry["elevation"])])
        responses.append(pair)
    points = np.array(directions).T
    cart = np.array([np.cos(points[0])*np.sin(points[1]), np.sin(points[0])*np.sin(points[1]), np.cos(points[1])]).T
    _, unique = np.unique(np.round(cart, 8), axis=0, return_index=True)
    unique.sort()
    return GridSphere(spherical_points=points[:,unique]), np.array(responses)[unique], digest.hexdigest()

def native_comparison(profile, helper, live_hrtf, measure):
    with tempfile.TemporaryDirectory(prefix="sda-room-reference-") as temporary:
        root = Path(temporary)
        source = root/"room.json"
        source.write_text(json.dumps(profile, separators=(",", ":"), allow_nan=False), encoding="utf-8")
        subprocess.run([helper, "reference", live_hrtf, str(root), str(source)], check=True)
        entries = json.loads((root/"reference.json").read_text(encoding="utf-8"))
        energy = {}
        for stage in ("direct", "early", "room"):
            selected = [entry for entry in entries if entry["stage"] == stage]
            if {e["speaker"] for e in selected} != {s["name"] for s in profile["speakers"]}:
                raise ValueError("Incomplete native room reference")
            pairs = [np.fromfile(root/e["file"], dtype="<f4").reshape(2, e["samplesPerEar"]) for e in selected]
            energy[stage] = measure(pairs)
        energy.update(hrtfhq=energy["direct"], raw=energy["direct"], calibrated=energy["direct"])
        reference = min(energy.values())
        return {"metric":"20 Hz-20 kHz pink-noise energy of native HRTFHQ room composition; equal independent feeds, not perceived loudness",
                "energyDb":energy, "gainDb":{k:max(-40,reference-v) for k,v in energy.items()},
                "limited":any(v-reference>40 for v in energy.values()), "basis":"native-mixed-speaker-v1"}
