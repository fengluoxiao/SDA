"""Run the production Kotlin validator against generated fixtures without Android or user audio uploads."""
from pathlib import Path
import argparse,os,shutil,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[1]
DEPENDENCIES=[('org.jetbrains.kotlin','kotlin-compiler-embeddable','2.3.10'),('org.jetbrains.kotlin','kotlin-stdlib','2.3.10'),('org.jetbrains.kotlin','kotlin-script-runtime','2.3.10'),('org.jetbrains.kotlin','kotlin-reflect','1.6.10'),('org.jetbrains.kotlin','kotlin-daemon-embeddable','2.3.10'),('org.jetbrains.kotlinx','kotlinx-coroutines-core-jvm','1.8.0'),('org.jetbrains','annotations','13.0')]
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('fixtures',type=Path);parser.add_argument('--original',type=Path);parser.add_argument('--java');parser.add_argument('--gradle-cache',type=Path,default=Path.home()/'.gradle/caches/modules-2/files-2.1');args=parser.parse_args()
 jars=[]
 for group,name,version in DEPENDENCIES:
  files=list((args.gradle_cache/group/name/version).glob('*/*.jar'))
  if len(files)!=1:raise RuntimeError(f'Resolve the Android Gradle dependencies first; missing or ambiguous {name}:{version}')
  jars.append(str(files[0]))
 java=args.java or (str(Path(os.environ['JAVA_HOME'])/'bin/java') if 'JAVA_HOME' in os.environ else shutil.which('java'))
 if not java:raise RuntimeError('Java 17 is required; provide --java or JAVA_HOME')
 with tempfile.TemporaryDirectory(prefix='sda-mp4-bounds-') as directory:
  subprocess.run([java,'-cp',os.pathsep.join(jars),'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler','-no-stdlib','-no-reflect','-jvm-target','17','-classpath',jars[1],'-d',directory,str(ROOT/'apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/Mp4AudioBounds.kt'),str(ROOT/'scripts/mp4-audio-bounds-probe.kt')],check=True)
  command=[java,'-cp',os.pathsep.join([directory,jars[1]]),'app.sda.mobile.sda.Mp4_audio_bounds_probeKt',str(args.fixtures)]
  if args.original:command.append(str(args.original))
  subprocess.run(command,check=True)
if __name__=='__main__':main()
