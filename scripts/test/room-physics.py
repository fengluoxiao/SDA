import contextlib
import importlib.util
import io
from pathlib import Path
import unittest
import numpy as np

spec=importlib.util.spec_from_file_location('simulator',Path(__file__).parents[1]/'room-simulator.py')
sim=importlib.util.module_from_spec(spec);spec.loader.exec_module(sim)

class PhysicsTests(unittest.TestCase):
    def test_source_faces_listener_at_all_layout_angles(self):
        for az in [-180,-140,-100,-90,-60,-45,-30,0,30,45,60,90,100,140,180]:
            for el in [0,45]:
                a,e=np.deg2rad([az,el])
                direction=-np.array([np.cos(a)*np.cos(e),np.sin(a)*np.cos(e),np.sin(e)])
                np.testing.assert_allclose(sim.source_orientation(direction).rotate([1,0,0]),direction,atol=1e-12)

    def test_early_reference_removes_only_late_reflections(self):
        direct=np.zeros(8192);direct[128]=1
        room=direct.copy();room[700]=.5;room[6000]=.25
        s={'directLeft':direct,'directRight':direct,'roomLeft':room,'roomRight':room,'onsetSample':128}
        early=sim.early_response(s)
        self.assertEqual(early[0,128],1)
        self.assertEqual(early[0,700],.5)
        self.assertEqual(early[0,6000],0)

    def test_geometry_and_actual_responses(self):
        config={'layout':'2.0','length':6,'width':4,'height':2.8,'earHeight':1.2,'placement':.85,'material':'treated','order':2,
                'speakers':[{'name':'FrontLeft','azimuth':30,'elevation':0},{'name':'FrontRight','azimuth':-30,'elevation':0}]}
        size,listener,positions=sim.geometry(config)
        for p in sim.paths_for(size,listener,positions[0]):
            distance=sum(np.linalg.norm(np.array(b)-a) for a,b in zip(p['points'],p['points'][1:]))
            self.assertAlmostEqual(distance,p['distance'],places=8)
            self.assertAlmostEqual(distance/343*1000,p['arrivalMs'],places=8)
        def run(c):
            with contextlib.redirect_stdout(io.StringIO()):
                return sim.simulate(c,'tmp/dirpat-loudspeakers.sofa','tmp/sadie-source/D1.zip','apps/web/public')
        base=run(config)
        hard=run({**config,'material':'reflective'})
        near=run({**config,'placement':.6})
        def energy(p):
            s=p['speakers'][0];n=min(len(s['roomLeft']),len(s['directLeft']))
            return np.sum((np.array(s['roomLeft'][:n])-np.array(s['directLeft'][:n]))**2)
        self.assertGreater(energy(hard),energy(base))
        # Material changes reflection attenuation, not direct sound.
        np.testing.assert_allclose(base['speakers'][0]['directLeft'],hard['speakers'][0]['directLeft'],atol=1e-7)
        self.assertLess(near['speakers'][0]['onsetSample'],base['speakers'][0]['onsetSample'])
        self.assertGreater(np.linalg.norm(near['speakers'][0]['directLeft']),np.linalg.norm(base['speakers'][0]['directLeft']))
        self.assertTrue(all(v<=0 for v in base['simulation']['comparison']['gainDb'].values()))

if __name__=='__main__':unittest.main()
