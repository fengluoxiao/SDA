"""Reuse the approved artwork fade curve as a small four-edge alpha mask."""
from pathlib import Path
import argparse
import numpy as np
from PIL import Image
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
ramp=np.asarray(Image.open(root/'apps/mobile/assets/artwork-color-fade-horizontal.png').convert('RGBA'))[0,:,3].astype(float)
size=256;span=16;distance=np.minimum(np.arange(size),np.arange(size)[::-1])
axis=np.interp(np.clip(distance/span,0,1)*(len(ramp)-1),np.arange(len(ramp)),ramp)
alpha=np.rint(axis[:,None]*axis[None,:]/255).astype('uint8')
rgba=np.full((size,size,4),255,dtype='uint8');rgba[:,:,3]=alpha
path=root/'apps/mobile/assets/artwork-edge-blend-mask.png'
assert alpha[0].max()==alpha[-1].max()==alpha[:,0].max()==alpha[:,-1].max()==0
assert np.all(alpha[span:-span,span:-span]==255)
assert np.array_equal(alpha,alpha[::-1,:]) and np.array_equal(alpha,alpha[:,::-1])
if args.check:
 assert np.array_equal(np.asarray(Image.open(path).convert('RGBA')),rgba)
 print('Artwork edge mask matches the existing fade curve; center opaque, edges transparent, both axes symmetric.')
else:Image.fromarray(rgba).save(path,optimize=True)
