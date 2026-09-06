"""Generate binaural room responses using measured Genelec/KU100 directivities."""
import argparse
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import wave
import zipfile

# Windows portable Python needs the wheel's BLAS DLL directories explicitly.
_dll_handles = []
if os.name == "nt":
    for root in os.environ.get("PYTHONPATH", "").split(os.pathsep):
        for name in ("scipy.libs", "numpy.libs"):
            directory = Path(root) / name
            if directory.is_dir():
                _dll_handles.append(os.add_dll_directory(str(directory.resolve())))
import numpy as np
import h5py
from scipy.signal import resample_poly
import pyroomacoustics as pra
from pyroomacoustics.directivities import MeasuredDirectivity, Rotation3D
from pyroomacoustics.doa import GridSphere

RATE = 48000
SOURCE_HASH = "d0891fe5413d28c4ea94f422ab9683ef0501b6206c1bba75dacc1ee6f723a7ac"
HRTF_HASH = "e5b58f6479d90cbc692a75ac2ccb8ad4385d22544121300733d2ef70bf2339e2"
MATERIALS = {
    "treated": [.25, .40, .60, .70, .72, .72, .70],
    "living": [.10, .14, .22, .30, .38, .42, .45],
    "reflective": [.08, .09, .10, .12, .15, .20, .24],
}

def load_measurements(source, archive):
    if hashlib.sha256(Path(source).read_bytes()).hexdigest() != SOURCE_HASH:
        raise ValueError("DIRPAT source checksum mismatch")
    if hashlib.sha256(Path(archive).read_bytes()).hexdigest() != HRTF_HASH:
        raise ValueError("SADIE source checksum mismatch")
    with h5py.File(source) as f:
        source_ir = resample_poly(f["Data.IR"][0], 160, 147, axis=-1)
        # Same DIRPAT position-order repair as pyroomacoustics._read_dirpat.
        pos = f["ReceiverPosition"][:, :, 0].reshape(36, -1, 3).swapaxes(0, 1).reshape(-1, 3)
    source_grid = GridSphere(spherical_points=pos[:, :2].T)
    directions, irs = [], []
    with zipfile.ZipFile(archive) as z:
        for name in z.namelist():
            if "D1_HRIR_WAV/48K_24bit/" not in name or not name.endswith(".wav"):
                continue
            match = re.search(r"azi_(-?\d+),(\d+)_ele_(-?\d+),(\d+)", name)
            if not match:
                continue
            az, el = float(match[1]+"."+match[2]), float(match[3]+"."+match[4])
            with wave.open(io.BytesIO(z.read(name))) as w:
                if w.getframerate() != RATE or w.getnchannels() != 2 or w.getsampwidth() != 3:
                    raise ValueError("Unexpected SADIE PCM format")
                b = np.frombuffer(w.readframes(w.getnframes()), dtype=np.uint8).reshape(-1, 3)
                v = b[:, 0].astype(np.int32) | (b[:, 1].astype(np.int32) << 8) | (b[:, 2].astype(np.int32) << 16)
                v = (v ^ 0x800000) - 0x800000
                irs.append((v.reshape(-1, 2).T / 8388608).astype(np.float32))
                directions.append([math.radians(az), math.pi/2-math.radians(el)])
    points = np.array(directions).T
    cart = np.array([np.cos(points[0])*np.sin(points[1]),np.sin(points[0])*np.sin(points[1]),np.cos(points[1])]).T
    # SADIE repeats pole coordinates with different azimuth labels. Keep one
    # original response per unique position; Voronoi integration needs uniqueness.
    _, unique = np.unique(np.round(cart, 8), axis=0, return_index=True)
    unique.sort()
    receiver_grid = GridSphere(spherical_points=points[:,unique])
    receiver_ir = np.array(irs)[unique]
    return source_grid, source_ir, receiver_grid, receiver_ir

def geometry(config):
    size = np.array([config["length"], config["width"], config["height"]], float)
    listener = np.array([size[0]/2, size[1]/2, config["earHeight"]])
    positions = []
    for speaker in config["speakers"]:
        az, el = np.deg2rad([speaker["azimuth"], speaker["elevation"]])
        ray = np.array([math.cos(az)*math.cos(el), math.sin(az)*math.cos(el), math.sin(el)])
        room_edge = np.where(ray >= 0, size-listener, listener)-.25
        radius = np.min(np.divide(room_edge, np.abs(ray), out=np.full(3, np.inf), where=np.abs(ray)>1e-8))
        positions.append(listener+ray*radius*config["placement"])
    return size, listener, positions

def paths_for(size, listener, source):
    distance = float(np.linalg.norm(source-listener))
    result = [{"wall":"direct","order":0,"distance":distance,"arrivalMs":distance/343*1000,"points":[source.tolist(),listener.tolist()]}]
    for axis, labels in enumerate([["back","front"],["right","left"],["floor","ceiling"]]):
        for value, wall in zip([0, size[axis]], labels):
            image = source.copy(); image[axis] = 2*value-source[axis]
            t = (value-listener[axis])/(image[axis]-listener[axis])
            point = listener+t*(image-listener)
            distance = float(np.linalg.norm(image-listener))
            result.append({"wall":wall,"order":1,"distance":distance,"arrivalMs":distance/343*1000,"points":[source.tolist(),point.tolist(),listener.tolist()]})
    return result

def pink_reference(pairs):
    frequency = np.fft.rfftfreq(65536, 1/RATE)
    weight = np.where((frequency >= 20) & (frequency <= 20000), 1/np.maximum(frequency,20), 0)
    power = [float(np.sum(np.abs(np.fft.rfft(a,n=65536))**2*weight)/weight.sum()) for pair in pairs for a in pair]
    return float(10*np.log10(np.mean(power)))

def source_orientation(direction):
    az = np.rad2deg(np.arctan2(direction[1], direction[0]))
    el = np.rad2deg(np.arctan2(direction[2], np.linalg.norm(direction[:2])))
    # Rotation3D uses extrinsic rotations: pitch the +X axis before yawing it.
    return Rotation3D([-el, az], rot_order="yz")

def early_response(speaker):
    direct = np.array([speaker["directLeft"], speaker["directRight"]])
    room = np.array([speaker["roomLeft"], speaker["roomRight"]])
    # Same complementary 10 ms transition as native cinema::Settings::mix.
    late = np.clip((np.arange(direct.shape[1])-speaker["onsetSample"]-50*48+240)/480, 0, 1)
    return direct + (room-direct)*(1-late)

def comparison_references(config, output, assets):
    result = {"room":pink_reference([(s["roomLeft"],s["roomRight"]) for s in output])}
    result["direct"] = pink_reference([(s["directLeft"],s["directRight"]) for s in output])
    result["early"] = pink_reference([early_response(s) for s in output])
    for mode, directory in [("raw","hrtf-raw"),("calibrated","hrtf")]:
        root = Path(assets)/directory
        manifest = json.loads((root/"hrtf-set.json").read_text(encoding="utf-8"))
        pairs = []
        for s in config["speakers"]:
            def vector(az,el):
                az,el=np.deg2rad([az,el]); return np.array([np.cos(az)*np.cos(el),np.sin(az)*np.cos(el),np.sin(el)])
            entry = max(manifest["positions"],key=lambda e:np.dot(vector(s["azimuth"],s["elevation"]),vector(e["azimuth"],e["elevation"])))
            dry=np.fromfile(root/entry["dry"],dtype="<f4").reshape(2,-1)
            wet=np.fromfile(root/entry["wet"],dtype="<f4").reshape(2,-1)
            d=np.pad(dry,((0,0),(0,wet.shape[1]-dry.shape[1])))
            pairs.append(d+.04*(wet-d))
        result[mode]=pink_reference(pairs)
    reference=min(result.values())
    return {"metric":"20 Hz-20 kHz pink-noise response energy, equal independent speaker feeds; not perceived loudness",
            "energyDb":result,"gainDb":{key:max(-40,reference-value) for key,value in result.items()},
            "limited":any(value-reference>40 for value in result.values())}

def simulate(config, source, archive, assets):
    sg, si, rg, ri = load_measurements(source, archive)
    size, listener, positions = geometry(config)
    receivers = [MeasuredDirectivity(Rotation3D([0, 0, 0]), rg, ri[:, ear, :], RATE) for ear in range(2)]
    material = pra.Material(energy_absorption={"coeffs":MATERIALS[config["material"]],"center_freqs":[125,250,500,1000,2000,4000,8000]})
    output, path_report = [], {}
    for index, (speaker, position) in enumerate(zip(config["speakers"], positions)):
        direction = listener-position
        directivity = MeasuredDirectivity(source_orientation(direction), sg, si, RATE)
        responses = []
        for order in [0, config["order"]]:
            room = pra.ShoeBox(size, fs=RATE, materials=material, max_order=order, air_absorption=True)
            room.set_sound_speed(343)
            room.add_source(position, directivity=directivity)
            for receiver in receivers:
                room.add_microphone(listener, directivity=receiver)
            room.compute_rir()
            responses.append([np.asarray(room.rir[ear][0]) for ear in range(2)])
        length = max(512, *(len(a) for pair in responses for a in pair))
        if length > 32768:
            raise ValueError("Simulated response exceeds 683 ms; lower reflection order or room size")
        entry = {"name":speaker["name"],"azimuth":speaker["azimuth"],"elevation":speaker["elevation"]}
        for key, a in zip(["directLeft","directRight","roomLeft","roomRight"], [*responses[0],*responses[1]]):
            if not np.isfinite(a).all():
                raise ValueError("Non-finite simulation output")
            entry[key] = np.pad(a, (0,length-len(a))).astype(np.float32).tolist()
        envelope = np.maximum(np.abs(entry["directLeft"]),np.abs(entry["directRight"]))
        entry["onsetSample"] = int(np.flatnonzero(envelope >= envelope.max()*.1)[0])
        output.append(entry)
        path_report[speaker["name"]] = paths_for(size,listener,position)
        print(json.dumps({"progress":index+1,"total":len(positions)}),flush=True)
    return {"version":1,"name":f"Genelec 8020 v2 / {config['layout']} / {config['length']}x{config['width']}x{config['height']} m / {config['material']}",
        "source":"DIRPAT Genelec 8020 measured directivity + SADIE II D1 KU100 measured HRIR; pyroomacoustics image-source simulation. Wall absorption is assumed; finite reflection order; source measurement window retained.",
        "license":"SADIE II Apache-2.0; DIRPAT publisher Public Domain Mark (embedded legacy license differs); local simulation derivative",
        "measurement":"simulated","sampleRate":RATE,"layout":config["layout"],"speakers":output,
        "simulation":{"engine":f"pyroomacoustics {pra.__version__}","revision":2,"config":config,"listener":listener.tolist(),"size":size.tolist(),
          "positions":{s["name"]:p.tolist() for s,p in zip(config["speakers"],positions)},"paths":path_report,
          "sourceSha256":SOURCE_HASH,"hrtfSha256":HRTF_HASH,"comparison":comparison_references(config,output,assets),
          "pathTimes":"geometric propagation only; measured source/HRIR latency is retained"}}

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True); parser.add_argument("--source", required=True)
    parser.add_argument("--hrtf", required=True); parser.add_argument("--output", required=True)
    parser.add_argument("--assets", default="apps/web/public")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    profile = simulate(config,args.source,args.hrtf,args.assets)
    Path(args.output).write_text(json.dumps(profile,separators=(",",":"),allow_nan=False),encoding="utf-8")
