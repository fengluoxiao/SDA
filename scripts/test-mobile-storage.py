from pathlib import Path
import os,subprocess,tempfile
root=Path(__file__).resolve().parents[1]
home=Path(os.environ.get('GRADLE_USER_HOME',str(Path.home()/'.gradle')))/'caches/modules-2/files-2.1'
java=Path(os.environ['JAVA_HOME'])/'bin/java.exe'
def jar(group,name,version=None):
 base=home/group/name
 if version is None:version=sorted((d.name for d in base.iterdir()),key=lambda v:tuple(int(x) if x.isdigit() else 0 for x in v.split('.')))[-1]
 return next((base/version).rglob('*.jar'))
compiler=jar('org.jetbrains.kotlin','kotlin-compiler-embeddable');version=compiler.parent.parent.name
stdlib=jar('org.jetbrains.kotlin','kotlin-stdlib',version)
classpath=[compiler,stdlib,jar('org.jetbrains.kotlin','kotlin-reflect'),jar('org.jetbrains.kotlinx','kotlinx-coroutines-core-jvm'),jar('org.jetbrains','annotations')]
with tempfile.TemporaryDirectory(prefix='sda-storage-') as temporary:
 out=Path(temporary);target=out/'storage-tests.jar'
 subprocess.run([str(java),'-cp',os.pathsep.join(map(str,classpath)),'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler','-no-stdlib','-no-reflect','-classpath',os.pathsep.join(map(str,[stdlib,classpath[-1]])),'-d',str(target),str(root/'apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/PlaybackStorage.kt'),str(root/'apps/mobile/test/native-queue/StorageRegression.kt')],check=True,cwd=root)
 subprocess.run([str(java),'-cp',os.pathsep.join(map(str,[target,stdlib])),'app.sda.mobile.sda.StorageRegressionKt',str(out)],check=True,cwd=root)
