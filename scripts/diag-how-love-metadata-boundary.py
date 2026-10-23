"""Audit replay metadata plus existing raw OAMD control evidence; offline only."""
import collections
import json
from pathlib import Path


def main():
    repo=Path(__file__).resolve().parents[1]
    downloads=Path.home()/'Downloads'
    fixtures=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63/frames.jsonl',55,63),
              ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65/frames.jsonl',0,65),
              ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay/frames.jsonl',0,138)]
    report={'scope':'Actual decoded replay fields, not bitstream completeness or voice semantics. EAC3 diffuse0 is expected: this field models MPEG-H diffuse energy, distinct from OAMD extent.', 'songs':[]}
    for name,path,start,end in fixtures:
        frames=[json.loads(line) for line in path.read_text().splitlines()]
        events=[e for f in frames for e in f['events'] if start*48000<=e['samplePos']<end*48000]
        objects={}
        for ident in sorted(set(e['id'] for e in events)):
            selected=[e for e in events if e['id']==ident]
            objects[str(ident)]={
                'eventCount':len(selected),
                'nonzeroExtentCount':sum(any(e.get('size',[0,0,0])) for e in selected),
                'nonzeroDiffuseCount':sum(e.get('diffuse',0)!=0 for e in selected),
                'finiteDistanceCount':sum(e.get('distanceM') is not None for e in selected),
                'infiniteDistanceCount':sum(e.get('distanceInfinite',False) for e in selected),
                'anchorCounts':dict(collections.Counter(e.get('anchor','room') for e in selected)),
                'nonzeroGainCount':sum(e['gainDb']!=0 for e in selected),
                'positionEventCount':sum(e['hasPos'] for e in selected),
                'positiveRampCount':sum(e.get('rampDuration',0)>0 for e in selected)}
        report['songs'].append({'name':name,'windowSecondsRelativeToFixture':[start,end],
                                'events':len(events),'objects':objects})
    raw=json.loads((repo/'tmp/how-love-diag/native-parity-2026-10-08/target-oamd-audit/summary.json').read_text())
    observations=[o for o in raw['observations'] if o['end_seconds']>55 and o['start_seconds']<63]
    names=['element1.warp_mode','element1.global_trim_mode','element1.disable_trim_per_object_present']
    counts={name:collections.Counter() for name in names}
    for observation in observations:
        oracle=observation['oamd'].get('oracle')
        if not isinstance(oracle,dict):
            continue
        for value in oracle.values():
            if not isinstance(value,list):
                continue
            for field in value:
                if isinstance(field,dict) and field.get('name') in counts:
                    counts[field['name']][str(field['integer_value'])]+=1
    assert all(sum(c.values())==len(observations) for c in counts.values())
    report['howTargetBitstreamAuditSummary']={
        'observations':len(observations),
        # Historical exporter named this field parse_error but stores the status
        # literal "accepted" on success. Do not count non-null as an error.
        'parseStatusCounts':dict(collections.Counter(str(o['parse_error']) for o in observations)),
        'vendorResultCounts':dict(collections.Counter(o['vendor_oamd']['result'] for o in observations)),
        'vendorTrimTimelineAvailableCount':sum(o['vendor_oamd'].get('trim_timeline_available',False) for o in observations),
        'rawTrimControlCounts':{name:dict(count) for name,count in counts.items()},
        'limits':'Raw controls are zero in this target window. Existing parser fidelity flag remains false; do not override certification or claim Apple equivalence. trim_config_count=9 is not evidence of nine nonzero adjustments.'}
    out=downloads/'SDA-clarity-validation/metadata-route-audit.json'
    out.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report['howTargetBitstreamAuditSummary'],indent=2))


if __name__=='__main__':
    main()
