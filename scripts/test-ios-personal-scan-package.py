"""Exercise resource-only IPA supplementation using isolated non-executable ZIPs."""
from pathlib import Path
import unittest,tempfile,zipfile,json,hashlib,os,subprocess,sys
ROOT=Path(__file__).resolve().parents[1]
class SupplementTests(unittest.TestCase):
 def fixture(self,root,signed=False):
  source=root/'base';source.mkdir();ipa=source/'fixture.ipa'
  with zipfile.ZipFile(ipa,'w')as z:
   z.writestr('Payload/SDA.app/SDA',b'NON-EXECUTABLE TEST FIXTURE')
   z.writestr('Payload/SDA.app/main.jsbundle',b'isolated fixture')
   if signed:z.writestr('Payload/SDA.app/_CodeSignature/CodeResources',b'reject')
  (source/'build-info.json').write_text(json.dumps({'sourceCommit':'a'*40,'appVersion':'0.9.40','appBuildNumber':'953','nativeDispatchStackRegression':{'passed':True}}))
  (source/'personal-scan-package-audit.json').write_text(json.dumps({'sha256':hashlib.sha256(ipa.read_bytes()).hexdigest(),'measurementOnly':True,'personalGenerationReady':False}))
  (source/'PERSONAL-HRTF-TEST-README-zh.md').write_text('Non-executable fixture')
  return source,ipa
 def run_job(self,source,out,commit='a'*40):
  env=dict(os.environ,SDA_BASE_SOURCE_COMMIT=commit,GITHUB_SHA='b'*40,SDA_BASE_RUN_ID='123')
  return subprocess.run([sys.executable,str(ROOT/'scripts/supplement-ios-personal-scan-notices.py'),str(source),str(out)],cwd=ROOT,env=env,capture_output=True,text=True)
 def test_original_entries_and_resource_provenance(self):
  with tempfile.TemporaryDirectory()as temp:
   root=Path(temp);source,base=self.fixture(root);out=root/'out'
   result=self.run_job(source,out);self.assertEqual(result.returncode,0,result.stderr)
   with zipfile.ZipFile(base)as a,zipfile.ZipFile(next(out.glob('*.ipa')))as b:
    for name in a.namelist():self.assertEqual(a.read(name),b.read(name))
    prefix='Payload/SDA.app/SdaCoreAssets.bundle/Licenses/ZipArchive/'
    metadata=json.loads(b.read(prefix+'provenance.json'))
    for name,digest in metadata['files'].items():self.assertEqual(hashlib.sha256(b.read(prefix+name)).hexdigest(),digest)
   info=json.loads((out/'build-info.json').read_text());self.assertEqual(info['sourceCommit'],'a'*40);self.assertEqual(info['resourcePackagingCommit'],'b'*40)
   self.assertTrue(info['resourceSupplementOnly'])
 def test_existing_identical_notices_preserved_without_duplicates(self):
  with tempfile.TemporaryDirectory()as temp:
   root=Path(temp);source,base=self.fixture(root)
   license_root=ROOT/'apps/mobile/modules/sda-core/ios/Licenses/ZipArchive'
   with zipfile.ZipFile(base,'a')as z:
    for p in license_root.rglob('*'):
     if p.is_file():z.write(p,'Payload/SDA.app/SdaCoreAssets.bundle/Licenses/ZipArchive/'+p.relative_to(license_root).as_posix())
   audit=source/'personal-scan-package-audit.json';record=json.loads(audit.read_text());record['sha256']=hashlib.sha256(base.read_bytes()).hexdigest();audit.write_text(json.dumps(record))
   out=root/'out';result=self.run_job(source,out);self.assertEqual(result.returncode,0,result.stderr)
   with zipfile.ZipFile(next(out.glob('*.ipa')))as z:self.assertEqual(len(z.namelist()),len(set(z.namelist())))
 def test_wrong_runtime_source_rejected(self):
  with tempfile.TemporaryDirectory()as temp:
   root=Path(temp);source,_=self.fixture(root)
   self.assertNotEqual(self.run_job(source,root/'out','c'*40).returncode,0)
 def test_signed_application_rejected(self):
  with tempfile.TemporaryDirectory()as temp:
   root=Path(temp);source,_=self.fixture(root,signed=True)
   self.assertNotEqual(self.run_job(source,root/'out').returncode,0)
if __name__=='__main__':unittest.main()
