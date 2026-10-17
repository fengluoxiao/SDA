"""Prepare diagnostic variants without dropping implicit speaker fallbacks.

Native loader infers the standard set from the dense directory name. A flat
temporary manifest changes the graph even with absolute PCM asset paths.
"""
import json
from pathlib import Path


def prepare_variant(asset, out, updates):
    asset, out = Path(asset).resolve(), Path(out).resolve()
    def absolute(value, base):
        if isinstance(value, dict):
            return {k:absolute(v,base) for k,v in value.items()}
        if isinstance(value, list):
            return [absolute(v,base) for v in value]
        if isinstance(value,str) and value.endswith('.f32') and (base/value).is_file():
            return str((base/value).resolve())
        return value
    directory = asset.parent.name
    matching = {'hrtf-dense':'hrtf','hrtf-dense-raw':'hrtf-raw'}
    standard = matching.get(directory)
    if standard is None and directory.endswith('-dense') and (directory.startswith('hrtf-h') or directory=='hrtf-d2-dense'):
        standard = directory[:-6]
    if standard:
        source = asset.parent.parent/standard/'hrtf-set.json'
        fallback = out/standard/'hrtf-set.json'
        fallback.parent.mkdir(parents=True,exist_ok=True)
        fallback.write_text(json.dumps(absolute(json.loads(source.read_text()),source.parent),indent=2),encoding='utf-8')
    target = out/directory/'hrtf-set.json'
    target.parent.mkdir(parents=True,exist_ok=True)
    manifest = absolute(json.loads(asset.read_text()),asset.parent)
    manifest['processing'].update(updates)
    target.write_text(json.dumps(manifest,indent=2),encoding='utf-8')
    return target
