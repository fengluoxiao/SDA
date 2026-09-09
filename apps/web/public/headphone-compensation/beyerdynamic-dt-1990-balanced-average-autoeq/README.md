# Beyerdynamic DT 1990 PRO, Balanced Earpads

First generation only. This profile does not represent Analytical earpads or MKII.
It applies the same average-measurement EQ to both channels, not per-unit or
independent left/right calibration.

Source: AutoEq, Rtings HMS II.3 over-ear, revision
7ae0f56d53074872b028649617a22bbb4232feb7.
https://github.com/jaakkopasanen/AutoEq/tree/7ae0f56d53074872b028649617a22bbb4232feb7/results/Rtings/HMS%20II.3%20over-ear/Beyerdynamic%20DT%201990%20(balanced%20earpads)

Upstream repository license: MIT. Attribution: Jaakko Pasanen / AutoEq,
measurement source Rtings. See the included LICENSE.txt.

Derived from the ten filters in ParametricEQ.txt (the README rounds source preamp
differently). Generated at 48 kHz with 8192 taps, normalized at 1 kHz. Source preamp
is excluded and safe output headroom is calculated again from the normalized
response. Rebuild with scripts/build-beyerdynamic-dt-1990-balanced-average-profile.mjs.
