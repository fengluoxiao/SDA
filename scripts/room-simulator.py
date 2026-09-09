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
from scipy.signal import resample_poly, sosfilt, butter
import pyroomacoustics as pra
from pyroomacoustics.directivities import MeasuredDirectivity, Rotation3D
from pyroomacoustics.doa import GridSphere

RATE = 48000
# The default whole-RIR zero-phase HPF leaks the tail into the direct window.
# Retain the documented ISM DC-artifact correction as one causal filter below.
# This generator runs in its own Python process; no host player setting changes.
pra.constants.set("rir_hpf_enable", False)
SOURCE_HASH = "d0891fe5413d28c4ea94f422ab9683ef0501b6206c1bba75dacc1ee6f723a7ac"
HRTF_HASH = "e5b58f6479d90cbc692a75ac2ccb8ad4385d22544121300733d2ef70bf2339e2"
MATERIAL_DATA = json.loads(Path(__file__).with_name("room-materials.json").read_text(encoding="utf-8"))
MATERIAL_ALIASES = {"treated":"rockwool_50mm_80kgm3", "living":"plasterboard", "reflective":"hard_surface"}

def material_spec(key):
    key = MATERIAL_ALIASES.get(key, key)
    if key == "studio":
        return key, {"description":"Near-field control room; per-surface literature materials", "coeffs":[0.0]*7}
    return key, MATERIAL_DATA["materials"][key]

def studio_surfaces():
    # Coverage is a design assumption. Area-weighted energy absorption is an
    # effective uniform wall, not spatially resolved absorber panels/diffusers.
    design = {"east":("panel_fabric_covered_6pcf",.7),
              "west":("rockwool_50mm_80kgm3",.85),
              "north":("rockwool_50mm_80kgm3",.85),
              "south":("rockwool_50mm_80kgm3",.85),
              "ceiling":("panel_fabric_covered_6pcf",.7),
              "floor":("carpet_1.35_kg_m2",1.0)}
    base=np.asarray(MATERIAL_DATA["materials"]["plasterboard"]["coeffs"])
    return {wall:{"materialId":key,"coverage":coverage,"remainder":"plasterboard",
                  "coeffs":(coverage*np.asarray(MATERIAL_DATA["materials"][key]["coeffs"])+(1-coverage)*base).tolist()}
            for wall,(key,coverage) in design.items()}

def studio_design_report(config, size, listener, positions, surfaces):
    areas={"east":size[1]*size[2],"west":size[1]*size[2],
           "north":size[0]*size[2],"south":size[0]*size[2],"floor":size[0]*size[1],"ceiling":size[0]*size[1]}
    area=sum(areas.values());volume=float(np.prod(size))
    mean=sum(areas[k]*np.asarray(v["coeffs"]) for k,v in surfaces.items())/area
    eyring=.161*volume/(-area*np.log1p(-mean))
    wall_names={"front":"east","back":"west","left":"north","right":"south","floor":"floor","ceiling":"ceiling"}
    checks=[]
    for speaker,position in zip(config["speakers"],positions):
        paths=paths_for(size,listener,position);direct=paths[0]
        for path in paths[1:]:
            delay=path["arrivalMs"]-direct["arrivalMs"]
            if delay<=15:
                alpha=np.asarray(surfaces[wall_names[path["wall"]]]["coeffs"])[3:]
                levels=20*np.log10(direct["distance"]/path["distance"]*np.sqrt(1-alpha))
                checks.append({"speaker":speaker["name"],"wall":path["wall"],"delayMs":delay,"worstDb":float(max(levels))})
    return {"basis":"EBU Tech 3276 acoustic targets; near-field layout is not its 2-4m stereo-base reference layout",
            "source":"https://tech.ebu.ch/docs/tech/tech3276.pdf",
            "nominalTargetSeconds":.25*(volume/100)**(1/3),"eyringSeconds":eyring.tolist(),
            "eyringNote":"diffuse-field design estimate, not measured T60 or low-frequency modal prediction",
            "firstOrderEarlyReflections":checks,"earlyTargetDb":-10,
            "earlyCheckNote":"geometric first-order 1-8kHz screening only; excludes HRTF/directivity, higher-order overlap and desk",
            "nearFieldDistanceMetres":config.get("listeningDistance",1.2),"listenerLengthFraction":.6}

def remove_ism_dc(values):
    # pyroomacoustics documents positive DC artifacts in image-source RIRs.
    # A causal, identical correction preserves direct/reflection superposition.
    return sosfilt(butter(2, 10.0, btype="highpass", fs=RATE, output="sos"), values)

def receiver_reference():
    return {"kind":"relative-digital", "propagationReferenceMetres":1.0,
            "pressureLaw":"1/r", "hrirMeasurementRadiusMetres":1.2,
            "hrirProcessing":"publisher diffuse-field equalized, time-aligned and windowed; preserved as supplied",
            "makeupGainDb":0.0, "absoluteSplCalibrated":False,
            "rirHighpassEnabled":False,
            "dcCorrection":"causal 2nd-order Butterworth 10 Hz; ISM numerical DC artifact correction, no gain normalization",
            "timing":"geometric r/c plus supplied FIR/filter latency; no second 1.2m flight time added",
            "source":"https://doi.org/10.3390/app8112029"}

def load_measurements(source, archive):
    if source != "ideal" and hashlib.sha256(Path(source).read_bytes()).hexdigest() != SOURCE_HASH:
        raise ValueError("DIRPAT source checksum mismatch")
    if hashlib.sha256(Path(archive).read_bytes()).hexdigest() != HRTF_HASH:
        raise ValueError("SADIE source checksum mismatch")
    source_grid, source_ir = None, None
    if source != "ideal":
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
    studio=config["material"]=="studio"
    listener = np.array([size[0]*(.6 if studio else .5), size[1]/2, config["earHeight"]])
    positions = []
    for speaker in config["speakers"]:
        az, el = np.deg2rad([speaker["azimuth"], speaker["elevation"]])
        ray = np.array([math.cos(az)*math.cos(el), math.sin(az)*math.cos(el), math.sin(el)])
        room_edge = np.where(ray >= 0, size-listener, listener)-.25
        radius = np.min(np.divide(room_edge, np.abs(ray), out=np.full(3, np.inf), where=np.abs(ray)>1e-8))
        distance=config.get("listeningDistance",1.2) if studio else radius*config["placement"]
        if distance>radius:raise ValueError("Listening distance places a monitor too close to/outside a room boundary")
        positions.append(listener+ray*distance)
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
    material_key, material = material_spec(config["material"])
    config = {**config, "material":material_key}
    sg, si, rg, ri = load_measurements(source, archive)
    size, listener, positions = geometry(config)
    receivers = [MeasuredDirectivity(Rotation3D([0, 0, 0]), rg, ri[:, ear, :], RATE) for ear in range(2)]
    surfaces=studio_surfaces() if material_key=="studio" else None
    material_info=material
    material = ({wall:pra.Material(energy_absorption={"coeffs":v["coeffs"],"center_freqs":MATERIAL_DATA["center_freqs"]}) for wall,v in surfaces.items()}
        if surfaces else pra.Material(energy_absorption={"coeffs":material["coeffs"],"center_freqs":MATERIAL_DATA["center_freqs"]}))
    output, path_report = [], {}
    for index, (speaker, position) in enumerate(zip(config["speakers"], positions)):
        direction = listener-position
        directivity = None if source == "ideal" else MeasuredDirectivity(source_orientation(direction), sg, si, RATE)
        responses = []
        for order in [0, config["order"]]:
            room = pra.ShoeBox(size, fs=RATE, materials=material, max_order=order, air_absorption=True)
            room.set_sound_speed(343)
            room.add_source(position, directivity=directivity)
            for receiver in receivers:
                room.add_microphone(listener, directivity=receiver)
            room.compute_rir()
            responses.append([np.asarray(room.rir[ear][0]) for ear in range(2)])
        length = max(512, *(len(a) for pair in responses for a in pair)) + 8192
        if length > 32768:
            raise ValueError("Simulated response exceeds 683 ms; lower reflection order or room size")
        entry = {"name":speaker["name"],"azimuth":speaker["azimuth"],"elevation":speaker["elevation"]}
        for key, a in zip(["directLeft","directRight","roomLeft","roomRight"], [*responses[0],*responses[1]]):
            if not np.isfinite(a).all():
                raise ValueError("Non-finite simulation output")
            entry[key] = remove_ism_dc(np.pad(a, (0,length-len(a)))).astype(np.float32).tolist()
        envelope = np.maximum(np.abs(entry["directLeft"]),np.abs(entry["directRight"]))
        entry["onsetSample"] = int(np.flatnonzero(envelope >= envelope.max()*.1)[0])
        output.append(entry)
        path_report[speaker["name"]] = paths_for(size,listener,position)
        print(json.dumps({"progress":index+1,"total":len(positions)}),flush=True)
    ideal = source == "ideal"
    if surfaces:material_info={**material_info,"coeffs":np.mean([s["coeffs"] for s in surfaces.values()],axis=0).tolist()}
    return {"version":1,"name":f"{'SDA Near-field Control Room' if surfaces else 'SADIE KU100 Reference' if ideal else 'Genelec 8020 v2'} / {config['layout']} / {config['length']}x{config['width']}x{config['height']} m / {config['material']}",
        "source":("SADIE II D1 KU100 publisher-equalized HRIR; ideal omnidirectional source; pyroomacoustics image-source simulation. Geometry and uniform material coverage are design assumptions; material coefficients use the cited literature table. Relative digital reference, not absolute SPL." if ideal else "DIRPAT Genelec 8020 measured directivity + SADIE II equalized HRIR; finite-order simulation. Source recording calibration is not established; not an absolute SPL model."),
        "license":("Apache-2.0; SADIE II Copyright 2018 University of York; generated derivative" if ideal else "SADIE II Apache-2.0; DIRPAT publisher Public Domain Mark (embedded legacy license differs); local simulation derivative"),
        "measurement":"simulated","sampleRate":RATE,"layout":config["layout"],"speakers":output,
        "simulation":{"engine":f"pyroomacoustics {pra.__version__}","revision":5 if surfaces else 4,"config":config,"listener":listener.tolist(),"size":size.tolist(),
          **({"surfaces":surfaces,"studioDesign":studio_design_report(config,size,listener,positions,surfaces)} if surfaces else {}),
          "reference":receiver_reference(),
          "material":{"id":material_key,**material_info,"centerFreqs":MATERIAL_DATA["center_freqs"],"source":MATERIAL_DATA["source"],"reference":MATERIAL_DATA["reference"],"sourceSha256":MATERIAL_DATA["sourceSha256"],"coverage":"per-surface area-weighted coverage; 8kHz floor coefficient held at 4kHz" if surfaces else "all six surfaces; design assumption"},
          "positions":{s["name"]:p.tolist() for s,p in zip(config["speakers"],positions)},"paths":path_report,
          "sourceModel":"ideal-omnidirectional" if ideal else "DIRPAT-Genelec-8020", "sourceSha256":None if ideal else SOURCE_HASH,"hrtfSha256":HRTF_HASH,"comparison":comparison_references(config,output,assets),
          "pathTimes":"geometric propagation only; publisher-aligned HRIR/filter latency retained, not original measurement flight time"}}

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True); parser.add_argument("--source", required=True)
    parser.add_argument("--hrtf", required=True); parser.add_argument("--output", required=True)
    parser.add_argument("--assets", default="apps/web/public")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    profile = simulate(config,args.source,args.hrtf,args.assets)
    Path(args.output).write_text(json.dumps(profile,separators=(",",":"),allow_nan=False),encoding="utf-8")
