// Embedded Mesh2HRTF NumCalc v1.3.0 (EUPL-1.2); see vendor notice.
#include "EmbeddedRuntime.hpp"
namespace sda_numcalc {
thread_local Runtime *runtime=nullptr;
static std::tm *local_walltime(std::time_t now) {
 static std::mutex timeMutex;
 std::lock_guard<std::mutex> lock(timeMutex);
 if(auto value=std::localtime(&now))runtime->wallTime=*value;
 return &runtime->wallTime;
}
#include "Generated/NC_Macros.h"
#include "Generated/NC_TypeDefinition.h"
#include "Generated/NC_ConstantsVariables.h"
#include "Generated/NC_Arrays.h"
#include "Generated/NC_IntegrationConstants.h"
#include "Generated/State.inc"
thread_local ofstream NCout;
#include "Generated/NC_Main.cpp.inc"
#include "Generated/NC_3dFunctions.cpp.inc"
#include "Generated/NC_Addresses.cpp.inc"
#include "Generated/NC_CommonFunctions.cpp.inc"
#include "Generated/NC_EquationSystem.cpp.inc"
#include "Generated/NC_Input.cpp.inc"
#include "Generated/NC_PostProcessing.cpp.inc"

static bool contained(const std::filesystem::path &root,const std::filesystem::path &p) {
 auto r=root.begin(),q=p.begin();
 for(;r!=root.end();++r,++q)if(q==p.end() || *r!=*q)return false;
 return true;
}
std::string local_path(const char *name) {
 check_now();
 if(!name || !*name)throw Failure(1,"Empty acoustic job path");
 std::filesystem::path relative(name);
 if(relative.is_absolute() || relative.has_root_name())throw Failure(1,"Acoustic job paths must be relative");
 auto path=std::filesystem::weakly_canonical(runtime->work/relative);
 if(!contained(runtime->owner.root,path))throw Failure(1,"Acoustic job path escapes private job directory");
 return path.string();
}
FILE *local_open(const char *path,const char *mode) {
 std::string actual=local_path(path);
 // Boundary debug dumps are unnecessary for HRTFs and can reach gigabytes.
 // Retain only evaluation pressures used by the iPhone's HRIR conversion.
 const auto filename=std::filesystem::path(path).filename().string();
 bool discard=mode[0]=='w' && (filename=="pBoundary" || filename=="vBoundary" || filename=="vEvalGrid" || filename=="load");
#if defined(_WIN32)
 const char *sink="NUL";
#else
 const char *sink="/dev/null";
#endif
 FILE *f=std::fopen(discard?sink:actual.c_str(),mode);
 if(!f)throw Failure(1,"Cannot open acoustic job file: "+std::string(path));
 try{runtime->files.insert(f);}catch(...){std::fclose(f);throw;}
 return f;
}
int local_close(FILE *f) {
 if(!f || !runtime->files.erase(f))throw Failure(1,"Invalid acoustic job file handle");
 return std::fclose(f);
}
int local_mkdir(const char *name,int) {
 std::filesystem::create_directories(local_path(name));return 0;
}
}
extern "C" SdaHrtfSolver *sda_hrtf_solver_create(const char *path,uint64_t budget) {
 try {
  if(!path || budget<1024*1024 || budget>512ull*1024*1024)return nullptr;
  auto root=std::filesystem::canonical(path);
  if(!std::filesystem::is_directory(root))return nullptr;
  std::unique_ptr<SdaHrtfSolver> job(new SdaHrtfSolver);
  job->root=root;job->budget=budget;return job.release();
 }catch(...){return nullptr;}
}
extern "C" void sda_hrtf_solver_cancel(SdaHrtfSolver *job) {
 if(job)job->cancelled.store(true,std::memory_order_relaxed);
}
extern "C" void sda_hrtf_solver_destroy(SdaHrtfSolver *job) {delete job;}
extern "C" SdaHrtfSolverResult sda_hrtf_solver_run(SdaHrtfSolver *job,const char *work,int first,int last,int estimate,SdaHrtfProgress progress,void *context) {
 SdaHrtfSolverResult result{};
 auto error=[&](int code,const char *message){result.code=code;std::snprintf(result.message,sizeof(result.message),"%s",message);};
 if(!job || !work || first<1 || last<first || last>4096 || (estimate!=0 && estimate!=1)) {error(1,"Invalid native acoustic job arguments");return result;}
 if(sda_numcalc::runtime){error(1,"Native solver cannot be re-entered from a progress callback");return result;}
 std::lock_guard<std::mutex> lock(job->mutex);
 try {
  std::filesystem::path rel(work);
  if(rel.is_absolute() || rel.has_root_name())throw sda_numcalc::Failure(1,"Work directory must be job-relative");
  auto actual=std::filesystem::canonical(job->root/rel);
  if(!sda_numcalc::contained(job->root,actual) || !std::filesystem::is_directory(actual))throw sda_numcalc::Failure(1,"Invalid private work directory");
  sda_numcalc::Runtime rt(*job,actual);
  struct Restore {~Restore(){sda_numcalc::runtime=nullptr;sda_numcalc::activeState=nullptr;}} restore;
  sda_numcalc::runtime=&rt;rt.progress=progress;rt.callbackContext=context;
  try {
   sda_numcalc::check_now();
   sda_numcalc::NCout.clear();
   // Fresh heap state for every ear/run, including upstream formerly-static FMM state.
   std::unique_ptr<sda_numcalc::State> state(new sda_numcalc::State{});
   sda_numcalc::activeState=state.get();
   std::string f=std::to_string(first),l=std::to_string(last);
   char app[]="embedded-numcalc",start[]="-istart",end[]="-iend",est[]="-estimate_ram";
   char *argv[]={app,start,f.data(),end,l.data(),est};
   int code=sda_numcalc::embedded_main(estimate?6:5,argv);
   if(code)throw sda_numcalc::Failure(1,"Native acoustic solver returned an error");
   sda_numcalc::frequency_progress(last,state->numFrequencies_);
  }catch(const sda_numcalc::Failure &e){
    std::string message=e.what();
    if(e.code==1){
     std::ifstream log(actual/("NC"+std::to_string(first)+"-"+std::to_string(last)+".out"));
     if(log){std::ostringstream contents;contents<<log.rdbuf();auto tail=contents.str();if(tail.size()>420)tail=tail.substr(tail.size()-420);message+="\n"+tail;}
    }
    error(e.code,message.c_str());
   }
   catch(const std::bad_alloc &){error(3,"Insufficient memory for native acoustic solve");}
   catch(const std::exception &e){error(1,e.what());}
   catch(...){error(1,"Unknown native acoustic solver failure");}
  // Also releases allocations that upstream normally leaves to process exit.
  rt.clear();result.peak_array_bytes=rt.peak;result.remaining_array_bytes=rt.used;
 }catch(const sda_numcalc::Failure &e){error(e.code,e.what());}
  catch(const std::exception &e){error(1,e.what());}
  catch(...){error(1,"Cannot start native acoustic job");}
 return result;
}
