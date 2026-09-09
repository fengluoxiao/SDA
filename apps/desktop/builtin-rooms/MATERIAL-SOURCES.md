# Material and signal reference

Revisions 4 and 5 use the absorption table published by pyroomacoustics:
https://github.com/LCAV/pyroomacoustics/blob/master/pyroomacoustics/data/materials.json

The library attributes these coefficients to the annex of Michael Vorländer,
*Auralization: Fundamentals of Acoustics, Modelling, Simulation, Algorithms,
and Acoustic Virtual Reality*, Springer, first edition, 2008.

Values are literature material coefficients, not certificates for a specific
manufacturer or measurements of a named studio. Material coverage, geometry,
source placement and ideal source choice are explicit simulation assumptions.
Each profile records its coefficients, frequency bands, source URL and source
file SHA-256. HRTF provenance and license remain SADIE II / University of York,
Apache-2.0. SADIE HRIRs are publisher-equalized and time-aligned, not raw SPL.

The generator uses relative digital input and 1/r propagation, without loudness
normalization or an added 1.2 m measurement-distance correction. A causal 10 Hz
DC-artifact correction replaces the library's noncausal whole-response filter.
Absolute SPL and a named real-room match are not established.

Revision 5 is a designed near-field studio control room, 6 x 5 x 3.2 m,
with every monitor at 1.2 m. Front and ceiling use 70% fabric-covered 6 pcf
rockwool panel; rear and sides use 85% 50 mm / 80 kg/m³ rockwool; remaining
area uses plasterboard. The floor uses carpet 1.35 kg/m² on felt/foam.
Its 8 kHz coefficient is held at the published 4 kHz value, not measured.
Area-weighted absorption models effective uniform walls, not individual panels.
Coverage and distance are design assumptions. EBU Tech 3276 supplies acoustic
screening references, not certification of this intentionally dry near-field
design (its stereo base and some reverberation estimates are below EBU ranges).
https://tech.ebu.ch/docs/tech/tech3276.pdf
https://www.genelec.com/monitor-placement

pyroomacoustics copyright (C) 2015–2019 Robin Scheibler (MIT):

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
