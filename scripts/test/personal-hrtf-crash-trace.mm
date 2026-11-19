#include <execinfo.h>
#include <dlfcn.h>
#include <signal.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
static void meshCrash(int signal){
 void *frames[64];int count=backtrace(frames,64);
 dprintf(STDERR_FILENO,"SDA_MESH_CRASH signal=%d frames=%d\n",signal,count);
 for(int i=0;i<count;i++){
  Dl_info info={};dladdr(frames[i],&info);
  dprintf(STDERR_FILENO,"SDA_FRAME %d address=%p base=%p image=%s symbol=%s\n",i,frames[i],info.dli_fbase,info.dli_fname?info.dli_fname:"?",info.dli_sname?info.dli_sname:"?");
 }
 _Exit(128+signal);
}
extern "C" void sdaInstallMeshCrashTrace(){
 setbuf(stdout,NULL);setbuf(stderr,NULL);
 signal(SIGSEGV,meshCrash);signal(SIGBUS,meshCrash);signal(SIGABRT,meshCrash);signal(SIGILL,meshCrash);
}
