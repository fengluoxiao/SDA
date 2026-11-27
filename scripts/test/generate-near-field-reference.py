"""Independent low-frequency point-source/rigid-sphere reference (requires scipy).

Duda & Martens, JASA 104, 3048 (1998), doi:10.1121/1.423886.
The pressure ratio uses the full frequency-domain spherical-wave solution,
not the production code's closed-form zero-frequency limit. The unneeded
common propagation phase and 1/(ka)^2 factor cancel in each distance ratio.
No measured human HRTF data or third-party code are redistributed.
"""
import json
from pathlib import Path

import numpy as np
from scipy.special import eval_legendre, spherical_jn, spherical_yn

ROOT = Path(__file__).resolve().parents[2]
HEAD_RADIUS = 0.0875
FREQUENCY = 20.0


def pressure(radius, lateral, terms):
    order = np.arange(terms)
    k = 2 * np.pi * FREQUENCY / 343.0
    incident = spherical_jn(order, k * radius) + 1j * spherical_yn(order, k * radius)
    derivative = spherical_jn(order, k * HEAD_RADIUS, derivative=True) + 1j * spherical_yn(
        order, k * HEAD_RADIUS, derivative=True
    )
    return radius * abs(np.sum((2 * order + 1) * eval_legendre(order, lateral) * incident / derivative))


def gains(radius, lateral, terms):
    response = np.array([
        pressure(radius, ear * lateral, terms) / pressure(1.0, ear * lateral, terms)
        for ear in (-1, 1)
    ])
    return response * np.sqrt(2.0 / np.sum(response**2))


rows = []
for radius in (0.12, 0.15, 0.2, 0.4, 0.6, 0.9, 0.999, 1.0):
    for lateral in (-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0):
        reference = gains(radius, lateral, 48)
        assert np.max(np.abs(reference - gains(radius, lateral, 64))) < 1e-7
        rows.append({"distanceM": radius, "lateral": lateral, "gains": reference.tolist()})

result = {
    "model": "rigid-sphere-point-source-neumann-boundary",
    "reference": "https://doi.org/10.1121/1.423886",
    "headRadiusM": HEAD_RADIUS,
    "referenceDistanceM": 1.0,
    "frequencyHz": FREQUENCY,
    "rows": rows,
}
output = ROOT / "apps/native-renderer/assets/near-field/sphere-reference.json"
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(f"Wrote {len(rows)} converged independent spherical-wave reference pairs: {output}")
