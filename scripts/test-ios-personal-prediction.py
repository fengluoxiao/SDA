from pathlib import Path
import os,subprocess,tempfile,sys
root=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory()as tmp:
 exe=Path(tmp)/('prediction.exe'if os.name=='nt'else'prediction')
 subprocess.run([os.environ.get('CXX','c++'),'-std=c++17','-O1','-I'+str(root/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative'),'-I'+sys.argv[1],str(root/'scripts/test/personal-hrtf-tracking-prediction.cpp'),'-o',str(exe)],check=True)
 subprocess.run([str(exe)],check=True)
