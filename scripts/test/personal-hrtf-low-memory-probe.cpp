// Run one actual reciprocal source/frequency in a DISPOSABLE copy of a NumCalc project.
// Array peak is scoped to NumCalc; process peak also includes untracked/OS allocations.
#include "SdaHrtfSolver.h"
#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>
#ifdef _WIN32
#include <windows.h>
#include <psapi.h>
#else
#include <sys/resource.h>
#endif

static uint64_t peakResidentBytes() {
#ifdef _WIN32
 PROCESS_MEMORY_COUNTERS c{};
 return GetProcessMemoryInfo(GetCurrentProcess(),&c,sizeof(c)) ? c.PeakWorkingSetSize : 0;
#else
 struct rusage u{}; if (getrusage(RUSAGE_SELF,&u)) return 0;
#ifdef __APPLE__
 return u.ru_maxrss;
#else
 return uint64_t(u.ru_maxrss)*1024;
#endif
#endif
}

int main(int argc,char **argv) try {
 namespace fs=std::filesystem;
 if(argc<5 || argc>7)throw std::runtime_error("Pass fresh project copy, frequency index, available MiB, physical MiB [--preflight] [--right]");
 bool preflight=false;std::string ear="NumCalc/source_1";
 for(int i=5;i<argc;++i){std::string arg=argv[i];if(arg=="--right")ear="NumCalc/source_2";else if(arg=="--preflight")preflight=true;else throw std::runtime_error("Unknown probe option");}
 fs::path root=fs::canonical(argv[1]);
 const int k=std::stoi(argv[2]),available=std::stoi(argv[3]),physical=std::stoi(argv[4]);
 if(k<1||k>4096||available<=0||physical<=0)throw std::invalid_argument("Invalid probe parameters");
 const uint64_t mib=1024*1024,budget=sda_hrtf_solve_budget(available*mib,physical*mib);
 fs::path out=root/ear/"be.out"/("be."+std::to_string(k))/"pEvalGrid";
 if(fs::exists(out))throw std::runtime_error("Stale pressure file: use a fresh copy of the input project");
 using Job=std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)>;
 Job job(sda_hrtf_create_for_resources(root.string().c_str(),available*mib,physical*mib),sda_hrtf_solver_destroy);
 if(!job)throw std::runtime_error("No solver budget or invalid job root");
 const auto t=std::chrono::steady_clock::now();
 auto r=preflight ? sda_hrtf_check_frequency_capacity(job.get(),ear.c_str(),k) : sda_hrtf_solver_run(job.get(),ear.c_str(),k,k,0,nullptr,nullptr);
 bool pressure=false;
 if(!r.code && !preflight){std::ifstream f(out);std::string header;int groups,group,n;
  if(std::getline(f,header)&&header.find("Mesh2HRTF 1.0.0")==0&&f>>groups>>group>>n&&groups==1&&group==1&&n>0&&n<=2048){
   bool finite=true,nonzero=false;for(int i=0;i<n;++i){int id;double re,im;if(!(f>>id>>re>>im)||!std::isfinite(re)||!std::isfinite(im)){finite=false;break;}nonzero|=re!=0||im!=0;}
   f>>std::ws;pressure=finite&&nonzero&&f.eof();
  }
 }
 job.reset();
 std::cout<<"frequencyIndex="<<k<<" preflight="<<preflight<<" budgetBytes="<<budget<<" code="<<r.code
          <<" trackedPeakBytes="<<r.peak_array_bytes<<" processPeakBytes="<<peakResidentBytes()
          <<" remainingTrackedBytes="<<r.remaining_array_bytes<<" validPressure="<<pressure
          <<" seconds="<<std::chrono::duration<double>(std::chrono::steady_clock::now()-t).count()
          <<" message="<<std::quoted(r.message)<<std::endl;
 if(preflight && fs::exists(out))throw std::runtime_error("Capacity preflight wrote a pressure file");
 return r.code ? r.code : ((preflight || pressure) && !r.remaining_array_bytes ? 0 : 1);
} catch(const std::exception &e){std::cerr<<e.what()<<std::endl;return 1;}
