// Embedded Mesh2HRTF NumCalc v1.3.0 (EUPL-1.2); see vendor notice.
#include "EmbeddedRuntime.hpp"
#if defined(__APPLE__)
#include <TargetConditionals.h>
#if TARGET_OS_IOS
#include <os/proc.h>
#endif
#endif
extern "C" uint64_t sda_hrtf_available_memory(void) {
#if defined(__APPLE__) && TARGET_OS_IOS
 return uint64_t(os_proc_available_memory());
#else
 return 0;
#endif
}
namespace { constexpr uint64_t mib=1024*1024, solveLimit=4096*mib; }
extern "C" uint64_t sda_hrtf_solve_budget(uint64_t headroom,uint64_t physical) {
 if(headroom<512*mib || physical<512*mib)return 0;
 const auto budget=std::min({solveLimit,physical/8,(headroom-256*mib)/5*3});
 return budget>=64*mib?budget:0;
}
extern "C" uint64_t sda_hrtf_recommended_budget(uint64_t headroom,uint64_t physical) {
 return std::min(768*mib,sda_hrtf_solve_budget(headroom,physical));
}
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
static constexpr const char *discardPath=
#if defined(_WIN32)
 "NUL";
#else
 "/dev/null";
#endif
static std::string log_path(const char *name){return runtime->capacityOnly ? discardPath : local_path(name);}
static int index_product(int a,int b){
 if(a<0 || b<0 || (b && a>std::numeric_limits<int>::max()/b))throw Failure(3,"FMM index product exceeds solver dimensions");
 return a*b;
}
static int index_sum(int a,int b){
 if(a<0 || b<0 || a>std::numeric_limits<int>::max()-b)throw Failure(3,"FMM index sum exceeds solver dimensions");
 return a+b;
}
#include "SdaHrtfCompactFar.hpp"
// T/S coefficients on every level and the trunk D translation coefficients
// coexist in the solve. Original clusters partition DOFs; reflected clusters
// contribute to T, not S. Omit lower-level D, near field, indices and scratch:
// this is a lower bound, not a forecast of RSS or a guarantee on success.
static void check_coefficient_storage(){
 const auto &s=state();uint64_t entries=0,largest=0,packedBytes=0;
 auto add=[&](uint64_t count){largest=std::max(largest,count);entries+=count;};
 auto level=[&](const ElCluster *clusters,int total,int original,int points,bool trunk){
  uint64_t t=0,sRows=0,d=0;
  for(int i=0;i<total;++i){
   if((i&255)==0)check_now();
   t+=clusters[i].NumOfDOFs;
   if(i<original){sRows+=clusters[i].NumOfDOFs;if(trunk)d+=clusters[i].NumFarClus;}
  }
  add(t*points);add(sRows*points);add(d*points);
  packedBytes+=(t+d)*sizeof(QPair)*uint64_t(points)/2;
 };
 if(s.methodFMM_==0)add(uint64_t(s.numRowsOfCoefficientMatrix_)*s.numRowsOfCoefficientMatrix_);
 else if(s.methodFMM_==1)level(s.clustarry,s.numOriginalReflectedClusters_,s.numOriginalClusters_,s.numIntegrationPointsUnitSphere_,true);
 else if(s.methodFMM_==3)for(int l=0;l<s.numClusterLevels_;++l){const auto &v=s.clulevarry[l];level(v.ClastArLv,v.nClustSLv,v.nClustOLv,v.nPoinSpheLv,l==0);}
 const uint64_t bytes=entries*sizeof(Complex), mib=1024*1024;
 runtime->checkedFrequency=true;
 runtime->compactFar=bytes>runtime->owner.budget*2/3 && compact_compatible();
 if(runtime->compactFar){
  if(packedBytes>runtime->owner.budget)throw Failure(3,"Reciprocal far-field coefficients exceed numerical budget");
  runtime->jobLog<<"Bounded reciprocal ML-FMM at "<<s.frequency_<<" Hz\n";
  return;
 }
 if(bytes>runtime->owner.budget)throw Failure(3,"At "+std::to_string(s.frequency_)+" Hz mandatory coefficient storage alone needs at least "+std::to_string((bytes+mib-1)/mib)+" MiB; numerical budget "+std::to_string(runtime->owner.budget/mib)+" MiB");
 if(largest*sizeof(Complex)>1024*mib)throw Failure(3,"Single mandatory coefficient array exceeds the 1 GiB limit");
}
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
 FILE *f=std::fopen(discard?discardPath:actual.c_str(),mode);
 if(!f)throw Failure(1,"Cannot open acoustic job file: "+std::string(path));
 try{runtime->files.insert(f);}catch(...){std::fclose(f);throw;}
 return f;
}
int local_close(FILE *f) {
 if(!f || !runtime->files.erase(f))throw Failure(1,"Invalid acoustic job file handle");
 return std::fclose(f);
}
int local_mkdir(const char *name,int) {
 if(!runtime->capacityOnly)std::filesystem::create_directories(local_path(name));
 return 0;
}
}
static SdaHrtfSolver *create_solver(const char *path,uint64_t budget) {
 try {
  if(!path || budget<mib || budget>solveLimit)return nullptr;
  auto root=std::filesystem::canonical(path);
  if(!std::filesystem::is_directory(root))return nullptr;
  std::unique_ptr<SdaHrtfSolver> job(new SdaHrtfSolver);
  job->root=root;job->budget=budget;return job.release();
 }catch(...){return nullptr;}
}
extern "C" SdaHrtfSolver *sda_hrtf_solver_create(const char *path,uint64_t budget) {
 return budget<=1024*mib ? create_solver(path,budget) : nullptr;
}
extern "C" SdaHrtfSolver *sda_hrtf_create_for_resources(const char *path,uint64_t headroom,uint64_t physical) {
 return create_solver(path,sda_hrtf_solve_budget(headroom,physical));
}
extern "C" void sda_hrtf_solver_cancel(SdaHrtfSolver *job) {
 if(job)job->cancelled.store(true,std::memory_order_relaxed);
}
extern "C" void sda_hrtf_solver_destroy(SdaHrtfSolver *job) {delete job;}
static SdaHrtfSolverResult run_frequency_range(SdaHrtfSolver *job,const char *work,int first,int last,int estimate,SdaHrtfProgress progress,void *context,bool capacityOnly) {
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
  sda_numcalc::Runtime rt(*job,actual);rt.capacityOnly=capacityOnly;
  struct Restore {~Restore(){sda_numcalc::runtime=nullptr;sda_numcalc::activeState=nullptr;}} restore;
  sda_numcalc::runtime=&rt;rt.progress=progress;rt.callbackContext=context;
  try {
   sda_numcalc::check_now();
   // Fresh heap state for every ear/run, including upstream formerly-static FMM state.
   std::unique_ptr<sda_numcalc::State> state(new sda_numcalc::State{});
   sda_numcalc::activeState=state.get();
   std::string f=std::to_string(first),l=std::to_string(last);
   char app[]="embedded-numcalc",start[]="-istart",end[]="-iend",est[]="-estimate_ram";
   char *argv[]={app,start,f.data(),end,l.data(),est};
   int code=sda_numcalc::embedded_main(estimate?6:5,argv);
   if(code)throw sda_numcalc::Failure(1,"Native acoustic solver returned an error");
   if(capacityOnly && !rt.checkedFrequency)throw sda_numcalc::Failure(1,"Frequency is outside the project's curve");
   sda_numcalc::frequency_progress(last,state->numFrequencies_);
  }catch(const sda_numcalc::Failure &e){
    if(rt.jobLog.is_open())rt.jobLog.close();
    std::string message=e.what();
    if(e.code==1 && !capacityOnly){
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
extern "C" SdaHrtfSolverResult sda_hrtf_solver_run(SdaHrtfSolver *job,const char *work,int first,int last,int estimate,SdaHrtfProgress progress,void *context){
 return run_frequency_range(job,work,first,last,estimate,progress,context,false);
}
extern "C" SdaHrtfSolverResult sda_hrtf_check_frequency_capacity(SdaHrtfSolver *job,const char *work,int index){
 return run_frequency_range(job,work,index,index,1,nullptr,nullptr,true);
}
