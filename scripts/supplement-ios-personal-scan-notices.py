"""Add dependency notices to an unsigned CI IPA without changing native code."""
from pathlib import Path
import sys,json,zipfile,hashlib,shutil,os
source,out=map(Path,sys.argv[1:]);out.mkdir(parents=True,exist_ok=True)
ipas=list(source.glob('*.ipa'));assert len(ipas)==1
base=ipas[0];info=json.loads((source/'build-info.json').read_text())
audit=json.loads((source/'personal-scan-package-audit.json').read_text())
assert hashlib.sha256(base.read_bytes()).hexdigest()==audit['sha256']
assert info['sourceCommit']==os.environ['SDA_BASE_SOURCE_COMMIT']
assert info['nativeDispatchStackRegression']['passed']
assert audit['measurementOnly'] and not audit['personalGenerationReady']
license_root=Path('apps/mobile/modules/sda-core/ios/Licenses/ZipArchive')
notice=json.loads((license_root/'provenance.json').read_text())
assert notice['commit']=='df35718ea19a94e015b91dc4881dee028ce4cdba'
for name,digest in notice['files'].items():
 assert hashlib.sha256((license_root/name).read_bytes()).hexdigest()==digest
ipa=out/('SDA-TrueDepth-measurement-'+info['appVersion']+'-build'+info['appBuildNumber']+'-unsigned-arm64.ipa')
shutil.copy2(base,ipa)
with zipfile.ZipFile(base)as z:
 assert 'Payload/SDA.app/_CodeSignature/CodeResources'not in z.namelist(),'Only unsigned app copies may receive notices'
 original={n:hashlib.sha256(z.read(n)).hexdigest()for n in z.namelist()if not n.endswith('/')}
with zipfile.ZipFile(ipa,'a',compression=zipfile.ZIP_DEFLATED)as z:
 for p in sorted(license_root.rglob('*')):
  if not p.is_file():continue
  name='Payload/SDA.app/SdaCoreAssets.bundle/Licenses/ZipArchive/'+p.relative_to(license_root).as_posix()
  if name in z.namelist():
   assert z.read(name)==p.read_bytes(),'Conflicting existing dependency notice'
  else:z.write(p,name)
with zipfile.ZipFile(ipa)as z:
 for name,digest in original.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest,name
info.update(resourcePackagingCommit=os.environ['GITHUB_SHA'],baseIpaSha256=audit['sha256'],baseRunID=os.environ['SDA_BASE_RUN_ID'],resourceSupplementOnly=True)
(out/'build-info.json').write_text(json.dumps(info,indent=2))
shutil.copy2(source/'PERSONAL-HRTF-TEST-README-zh.md',out/'PERSONAL-HRTF-TEST-README-zh.md')
if(source/'device-uuid.txt').exists():shutil.copy2(source/'device-uuid.txt',out/'device-uuid.txt')
(out/'resource-supplement.json').write_text(json.dumps({'nativeSourceCommit':info['sourceCommit'],'packagingCommit':os.environ['GITHUB_SHA'],'originalEntriesPreserved':len(original),'dependency':notice,'unsigned':True},indent=2))
print('Dependency notices added; all original IPA entries byte-identical.')
