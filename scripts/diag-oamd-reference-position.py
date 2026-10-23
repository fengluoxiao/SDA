"""Compare captured OpenJOC debug positions to native replay metadata (offline).

This audit deliberately covers only captured frames, not uncaptured target audio.
Row ordinal correspondence is diagnostic, not original authored identity proof.
"""
import argparse
import json
import re
from collections import Counter
from pathlib import Path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--debug', type=Path, required=True)
    p.add_argument('--frames', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--frame-offset', type=int, default=0,
                   help='Original AU index of the first frame in the diagnostic excerpt')
    a = p.parse_args()
    native = [json.loads(line) for line in a.frames.read_text(encoding='utf-8').splitlines()]
    report = {'scope': 'Captured debug frames only; not full target-window certification',
              'productionChanged': False, 'comparisons': [], 'errors': []}
    controls = Counter()
    pattern = re.compile(r'position: Position3\s*\{\s*x:\s*([-\d.eE]+),\s*y:\s*([-\d.eE]+),\s*z:\s*([-\d.eE]+),')
    for directory in sorted(a.debug.glob('frame_*')):
        index = int(directory.name.split('_')[-1]) + a.frame_offset
        text = (directory/'oamd.txt').read_text(encoding='utf-8')
        positions = [tuple(map(float, xyz)) for xyz in pattern.findall(text)]
        gains = [int(x) for x in re.findall(r'gain: Decibels\(\s*(-?\d+),', text)]
        timing = re.findall(r'start_sample:\s*(\d+),\s*ramp_duration:\s*(\d+),', text)
        offset = re.search(r'sample_offset:\s*(\d+),', text)
        for field, pat in [('channelLock', r'channel_lock: (\w+)'),
                           ('zone', r'zones: \[([^\]]*)\]'),
                           ('warp', r'warp_mode: (\w+)'),
                           ('extent', r'size: Extent3 \{([^}]+)')]:
            for value in re.findall(pat, text):
                controls[field+':'+re.sub(r'\s+', '', value)] += 1
        # For this admitted fixture exactly one update block and LFE at index0.
        events = native[index]['events']
        if len(positions) != 16 or len(events) != 15 or len(gains) != 16 or len(timing) != 1 or offset is None:
            report['errors'].append({'frame': index, 'reason': 'Fixture dimensions/update count mismatch'})
            continue
        for ordinal, xyz in enumerate(positions[1:]):
            event = events[ordinal]
            if event['id'] != ordinal+10:
                report['errors'].append({'frame': index, 'reason': 'Unexpected ordinal ordering'})
                continue
            expected = [2*xyz[0]-1, 1-2*xyz[1], xyz[2]]
            err = max(abs(x-y) for x, y in zip(expected, event['pos']))
            start, ramp = map(int, timing[0])
            timing_error = event['samplePos'] - (index*1536+int(offset[1])+start+577)
            if event['gainDb'] != gains[ordinal+1] or event['rampDuration'] != ramp or timing_error:
                report['errors'].append({'frame': index, 'ordinal': ordinal,
                                        'reason': 'Gain/ramp/sample timing differs'})
            report['comparisons'].append({'frame': index, 'ordinal': ordinal,
                                          'maxCoordinateError': err})
    if not report['comparisons']:
        raise RuntimeError('No comparable metadata; cannot pass')
    report['maxCoordinateError'] = max(c['maxCoordinateError'] for c in report['comparisons'])
    report['comparedFrames'] = len(set(c['frame'] for c in report['comparisons']))
    report['controlInventory'] = dict(controls)
    report['passed'] = not report['errors'] and report['maxCoordinateError'] < 1e-6
    a.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k not in ('comparisons', 'errors')}))


if __name__ == '__main__':
    main()
