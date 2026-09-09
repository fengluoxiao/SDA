import contextlib
import importlib.util
import io
from pathlib import Path
import unittest
import numpy as np

spec=importlib.util.spec_from_file_location('simulator',Path(__file__).parents[1]/'room-simulator.py')
sim=importlib.util.module_from_spec(spec);spec.loader.exec_module(sim)

class PhysicsTests(unittest.TestCase):
    def test_relative_distance_law_without_makeup(self):
        # Integer propagation times at 48 kHz keep fractional-delay differences
        # out of this check. Same source strength; no normalize() or output gain.
        def direct(distance):
            room=sim.pra.ShoeBox([12,8,5],fs=sim.RATE,max_order=0,air_absorption=False)
            room.set_sound_speed(343)
            room.add_source([2,2,2]);room.add_microphone([2+distance,2,2])
            room.compute_rir()
            return np.asarray(room.rir[0][0])
        near,far=direct(1.715),direct(3.43)
        ratio=np.linalg.norm(far)/np.linalg.norm(near)
        self.assertAlmostEqual(ratio,.5,places=5)
        self.assertEqual(int(np.argmax(abs(far)))-int(np.argmax(abs(near))),240)
        self.assertEqual(sim.receiver_reference()['makeupGainDb'],0)

    def test_reflections_do_not_rescale_direct_arrival(self):
        def response(order,alpha):
            room=sim.pra.ShoeBox([8,8,5],fs=sim.RATE,max_order=order,
                materials=sim.pra.Material(alpha),air_absorption=False)
            room.set_sound_speed(343)
            room.add_source([3,3,2]);room.add_microphone([4,3,2])
            room.compute_rir();return np.asarray(room.rir[0][0])
        direct=response(0,.2)
        for alpha in [.1,.8]:
            full=response(1,alpha)
            np.testing.assert_allclose(full[:len(direct)],direct,atol=1e-6)
            # Production DC correction must be causal too: adding a future
            # reflection cannot alter the corrected direct arrival.
            np.testing.assert_allclose(sim.remove_ism_dc(full)[:len(direct)],
                sim.remove_ism_dc(direct),atol=1e-6)
            self.assertGreater(np.linalg.norm(full[len(direct):]),0)
        self.assertGreater(np.linalg.norm(response(1,.1)),np.linalg.norm(response(1,.8)))

    def test_materials_match_installed_literature_table(self):
        for key,spec in sim.MATERIAL_DATA['materials'].items():
            published=sim.pra.materials_absorption_table[key]['coeffs']
            self.assertEqual(spec['coeffs'][:len(published)],published)
            if len(published)<7:
                self.assertIn('extrapolation',spec)
                self.assertEqual(spec['coeffs'][len(published):],[published[-1]]*(7-len(published)))
        self.assertEqual(sim.material_spec('treated')[0],'rockwool_50mm_80kgm3')

    def test_studio_geometry_and_early_design(self):
        config={'length':6,'width':5,'height':3.2,'earHeight':1.2,'material':'studio','listeningDistance':1.2,
            'speakers':[{'name':str(a),'azimuth':a,'elevation':e} for a,e in [(30,0),(-30,0),(0,0),(90,45),(-90,45)]]}
        size,listener,positions=sim.geometry(config)
        np.testing.assert_allclose([np.linalg.norm(p-listener) for p in positions],1.2)
        self.assertAlmostEqual(positions[0][1]+positions[1][1],2*listener[1])
        surfaces=sim.studio_surfaces()
        self.assertEqual(surfaces['north'],surfaces['south'])
        self.assertNotEqual(surfaces['floor']['coeffs'],surfaces['ceiling']['coeffs'])
        for s in surfaces.values():
            expected=s['coverage']*np.array(sim.MATERIAL_DATA['materials'][s['materialId']]['coeffs'])+(1-s['coverage'])*np.array(sim.MATERIAL_DATA['materials'][s['remainder']]['coeffs'])
            np.testing.assert_allclose(s['coeffs'],expected)
        report=sim.studio_design_report(config,size,listener,positions,surfaces)
        self.assertTrue(all(r['worstDb']<=-10 for r in report['firstOrderEarlyReflections']))
        with self.assertRaisesRegex(ValueError,'boundary'):
            sim.geometry({**config,'listeningDistance':4})
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
                return sim.simulate(c,'ideal','tmp/sadie-source/D1.zip','apps/web/public')
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
