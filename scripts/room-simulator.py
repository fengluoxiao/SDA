"""Generate room responses with the native-prepared shared HRTFHQ receiver."""
import argparse
from room_hrtfhq import load_receiver, native_comparison
import hashlib
import json
import math
import os
from pathlib import Path

# Windows portable Python needs the wheel's BLAS DLL directories explicitly.
_dll_handles = []
if os.name == "nt":
    for root in os.environ.get("PYTHONPATH", "").split(os.pathsep):
        for name in ("scipy.libs", "numpy.libs"):
            directory = Path(root) / name
            if directory.is_dir():
                _dll_handles.append(os.add_dll_directory(str(directory.resolve())))
import numpy as np
from scipy.signal import sosfilt, butter
import pyroomacoustics as pra
from pyroomacoustics.directivities import MeasuredDirectivity, Rotation3D
from pyroomacoustics.doa import GridSphere

RATE = 48000
# The default whole-RIR zero-phase HPF leaks the tail into the direct window.
# Retain the documented ISM DC-artifact correction as one causal filter below.
# This generator runs in its own Python process; no host player setting changes.
pra.constants.set("rir_hpf_enable", False)
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
            "hrirProcessing":"Native-prepared shared HRTFHQ v3 including common-ear clarity v1; one receiver filter per reflection path",
            "makeupGainDb":0.0, "absoluteSplCalibrated":False,
            "rirHighpassEnabled":False,
            "dcCorrection":"causal 2nd-order Butterworth 10 Hz; ISM numerical DC artifact correction, no gain normalization",
            "timing":"geometric r/c plus supplied FIR/filter latency; no second 1.2m flight time added",
            "source":"SDA shared apple-hrtfhq-542-v1 public synthetic transfer measurements; not personalized Spatial Audio"}


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
        distance=config.get("listeningDistance",1.2) if studio and config.get("shape") != "box" else radius*config["placement"]
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

def simulate(config, source, archive, assets):
    material_key, material = material_spec(config["material"])
    config = {**config, "material":material_key}
    if source != "ideal":
        raise ValueError("Room generation uses an ideal source and the shared HRTFHQ receiver")
    rg, ri, receiver_hash = load_receiver(archive)
    size, listener, positions = geometry(config)
    receivers = [MeasuredDirectivity(Rotation3D([0, 0, 0]), rg, ri[:, ear, :], RATE) for ear in range(2)]
    surfaces=studio_surfaces() if material_key=="studio" else None
    material_info=material
    material = ({wall:pra.Material(energy_absorption={"coeffs":v["coeffs"],"center_freqs":MATERIAL_DATA["center_freqs"]}) for wall,v in surfaces.items()}
        if surfaces else pra.Material(energy_absorption={"coeffs":material["coeffs"],"center_freqs":MATERIAL_DATA["center_freqs"]}))
    output, path_report = [], {}
    for index, (speaker, position) in enumerate(zip(config["speakers"], positions)):
        direction = listener-position
        directivity = None
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
    if surfaces:material_info={**material_info,"coeffs":np.mean([s["coeffs"] for s in surfaces.values()],axis=0).tolist()}
    return {"version":1,"name":f"SDA HRTFHQ Control Room / {config['layout']} / {config['length']}x{config['width']}x{config['height']} m / {config['material']}",
        "source":"Native-prepared SDA shared HRTFHQ v3 receiver with clarity v1; pyroomacoustics image-source simulation; geometric/material design assumptions; relative digital reference, not absolute SPL.",
        "license":"SDA synthetic public HRTFHQ transfer measurements; pyroomacoustics simulation derivative; see bundled provenance and license notices",
        "measurement":"simulated","sampleRate":RATE,"layout":config["layout"],"speakers":output,
        "simulation":{"engine":f"pyroomacoustics {pra.__version__}","revision":6 if surfaces else 4,"config":config,"listener":listener.tolist(),"size":size.tolist(),
          **({"surfaces":surfaces,"studioDesign":studio_design_report(config,size,listener,positions,surfaces)} if surfaces else {}),
          "reference":receiver_reference(),
          "material":{"id":material_key,**material_info,"centerFreqs":MATERIAL_DATA["center_freqs"],"source":MATERIAL_DATA["source"],"reference":MATERIAL_DATA["reference"],"sourceSha256":MATERIAL_DATA["sourceSha256"],"coverage":"per-surface area-weighted coverage; 8kHz floor coefficient held at 4kHz" if surfaces else "all six surfaces; design assumption"},
          "positions":{s["name"]:p.tolist() for s,p in zip(config["speakers"],positions)},"paths":path_report,
          "sourceModel":"ideal-omnidirectional", "sourceSha256":None, "hrtfSha256":receiver_hash, "hrtfProfile":"apple-hrtfhq-542-v1", "receiverPreparation":"native-shared-clarity-v1", "comparison":{},
          "pathTimes":"geometric propagation only; native-prepared HRIR/filter latency retained once, not measurement flight time"}}

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True); parser.add_argument("--source", choices=["ideal"], default="ideal")
    parser.add_argument("--hrtf", required=True); parser.add_argument("--output", required=True)
    parser.add_argument("--reference-helper", required=True); parser.add_argument("--live-hrtf", required=True)
    parser.add_argument("--assets", default="apps/web/public")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    profile = simulate(config,args.source,args.hrtf,args.assets)
    profile["simulation"]["comparison"] = native_comparison(profile, args.reference_helper, args.live_hrtf, pink_reference)
    Path(args.output).write_text(json.dumps(profile,separators=(",",":"),allow_nan=False),encoding="utf-8")
