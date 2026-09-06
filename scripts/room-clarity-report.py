"""Inspect measured source tails and separate simulated direct/reflected energy."""
import importlib.util
import json
from pathlib import Path
import sys

spec = importlib.util.spec_from_file_location("simulator", Path(__file__).with_name("room-simulator.py"))
sim = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sim)
np = sim.np

def energy_db(signal):
    return float(10 * np.log10(max(float(np.sum(np.square(signal))), 1e-30)))

def report(profile, source, archive):
    sg, si, _, _ = sim.load_measurements(source, archive)
    front = int(np.argmax(sg.cartesian[0]))
    impulse = si[front]
    peak = int(np.argmax(np.abs(impulse)))
    source_report = {"frontIndex": front, "peakMs": peak / 48,
                     "lengthMs": len(impulse) / 48,
                     "tailRelativeDb": {str(ms): energy_db(impulse[peak+int(ms*48):])-energy_db(impulse)
                                        for ms in [2, 5, 10, 20]}}
    rows = []
    for speaker in profile["speakers"]:
        direct = np.array([speaker["directLeft"], speaker["directRight"]])
        room = np.array([speaker["roomLeft"], speaker["roomRight"]])
        reflected = room - direct
        rows.append({"speaker": speaker["name"],
                     "reflectionToDirectDb": energy_db(reflected)-energy_db(direct),
                     "directPinkDb": sim.pink_reference([direct]),
                     "roomPinkDb": sim.pink_reference([room])})
    return {"source": source_report, "speakers": rows}

if __name__ == "__main__":
    profile = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    result = report(profile, sys.argv[2], sys.argv[3])
    print(json.dumps(result, indent=2))
