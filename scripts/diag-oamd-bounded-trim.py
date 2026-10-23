"""Offline bounded trim syntax audit; NEVER selects a playback configuration.

Presence ordering/LUTs: ETSI TS 103 420 V1.2.1 tables 34--39.
Configuration cardinality and output-layout applicability remain external inputs.
The supplied element includes the leading discard bit, unlike parse_trim_element.
"""
import argparse
import json
from pathlib import Path


class Bits:
    def __init__(self, text):
        if not text or any(c not in '01' for c in text):
            raise ValueError('Expected a nonempty binary element')
        self.text, self.pos = text, 0

    def read(self, n):
        if self.pos + n > len(self.text):
            raise ValueError(f'bounded overrun at {self.pos}, need {n}')
        result = int(self.text[self.pos:self.pos+n], 2)
        self.pos += n
        return result


CENTRE = [6, 3, 1.5, .75, -.75, -1.5, -3, -4.5,
          -6, -7.5, -9, -10.5, -12, -13.5, -16, -36]
SURROUND_HEIGHT = [None]*4 + [-.75, -1.5, -3, -4.5, -6, -7.5,
                             -9, -10.5, -12, -13.5, -16, -36]


def parse(text, count, objects=15):
    if not 1 <= count <= 16 or objects < 0:
        raise ValueError('Invalid diagnostic dimensions')
    b = Bits(text)
    result = {'count': count, 'configurations': [], 'error': None}
    try:
        result['discard'] = b.read(1)
        warp = result['warp'] = b.read(2)
        if warp > 1 or b.read(2):
            raise ValueError('Reserved warp or nonzero reserved bits')
        mode = result['globalMode'] = b.read(2)
        if mode == 3:
            raise ValueError('Reserved global trim mode')
        if mode == 2:
            for i in range(count):
                cfg = {'index': i}
                if b.read(1):
                    cfg['mode'] = 'default'
                elif b.read(1):
                    cfg['mode'] = 'disabled'
                else:
                    cfg['mode'] = 'custom'
                    presence = cfg['presence'] = b.read(5)
                    for mask, name, table in [(16, 'centreDb', CENTRE),
                                              (8, 'surroundDb', SURROUND_HEIGHT),
                                              (4, 'heightDb', SURROUND_HEIGHT)]:
                        if presence & mask:
                            code = b.read(4)
                            if table[code] is None:
                                raise ValueError('Reserved surround/height code')
                            cfg[name] = {'code': code, 'db': table[code]}
                    for mask, name in [(2, 'topBottomY'), (1, 'listenerY')]:
                        if presence & mask:
                            sign, amount = b.read(1), b.read(4)
                            cfg[name] = {'signCode': sign, 'amountCode': amount,
                                         'value': (1 if sign else -1)*(amount+1)/16}
                result['configurations'].append(cfg)
        present = result['perObjectDisablePresent'] = b.read(1)
        result['disabledObjects'] = [bool(b.read(1)) for _ in range(objects)] if present else []
        if '1' in b.text[b.pos:]:
            raise ValueError('Nonzero trailing padding')
    except ValueError as e:
        result['error'] = str(e)
    result['consumedBits'] = b.pos
    result['trailingBits'] = b.text[b.pos:]
    return result


def self_test():
    text = '00000101000011001111000111111100'
    eight = parse(text, 8)
    assert eight['error'] is None and eight['consumedBits'] == 31
    cfg = eight['configurations'][1]
    assert cfg['heightDb']['db'] == -4.5 and cfg['topBottomY']['value'] == .125
    assert 'centreDb' not in cfg and 'surroundDb' not in cfg
    assert parse(text, 9)['error'] is not None
    assert parse(text[:30], 8)['error'] is not None
    assert parse(text[:-1]+'1', 8)['error'] == 'Nonzero trailing padding'
    assert parse('00000000', 8)['error'] is None  # global default
    # Isolate every presence flag, including the two Y fields.
    for mask, key, payload in [(16, 'centreDb', '0000'), (8, 'surroundDb', '0111'),
                               (4, 'heightDb', '0111'), (2, 'topBottomY', '10001'),
                               (1, 'listenerY', '10001')]:
        synthetic = '0000010' + '00' + format(mask, '05b') + payload + '0'
        cfg = parse(synthetic, 1)['configurations'][0]
        assert key in cfg and cfg['presence'] == mask


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', type=Path, required=True, help='bounded-bits JSON')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    self_test()
    source = json.loads(a.input.read_text(encoding='utf-8-sig'))
    report = {'productionChanged': False, 'selfTestsPassed': True,
              'scope': 'Syntax only; no layout assignment or automatic cardinality selection',
              'elements': []}
    for text, observations in source['elementBitstrings'].items():
        trials = [parse(text, count) for count in range(1, 17)]
        report['elements'].append({'bits': text, 'observations': observations,
                                  'admittedCounts': [t['count'] for t in trials if t['error'] is None],
                                  'trials': trials})
    a.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({'selfTestsPassed': True, 'admittedCounts':
                      [e['admittedCounts'] for e in report['elements']]}))


if __name__ == '__main__':
    main()
