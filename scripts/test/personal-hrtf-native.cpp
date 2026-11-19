#include "SdaHrtfSolver.h"
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <stdexcept>
#include <vector>
#include <cmath>
#include <future>
#include <algorithm>
#include <limits>
#ifdef __APPLE__
#include <execinfo.h>
#endif
using Job=std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)>;
void require(bool good,const std::string &why){if(!good)throw std::runtime_error(why);}
Job job(const std::filesystem::path &root,uint64_t bytes=128ull*1024*1024){
 Job j(sda_hrtf_solver_create(root.string().c_str(),bytes),sda_hrtf_solver_destroy);require(bool(j),"Create native solver");return j;
}
std::vector<double> pressure(const std::filesystem::path &path){
 std::ifstream in(path);require(bool(in),"Missing pressure output");std::string line;
 for(int i=0;i<3;++i)std::getline(in,line);
 std::vector<double> result;int id;double re,im;
 while(in>>id>>re>>im){require(std::isfinite(re)&&std::isfinite(im),"Non-finite pressure");result.push_back(re);result.push_back(im);}
 require(result.size()==8,"Four complex pressure directions required");return result;
}
void check_result(SdaHrtfSolverResult r,int code,const std::string &label){
 require(r.code==code,label+": code="+std::to_string(r.code)+" "+r.message);
 require(r.remaining_array_bytes==0,label+": leaked native arrays");
}
void impulse_test(const std::filesystem::path &root,SdaHrtfSolver *job){
 std::vector<double> pressure(3*2*256*2);
 std::vector<float> expected(3*2*512),output(2*512);
 std::ifstream a(root/"fft-pressure-f64.bin",std::ios::binary),b(root/"fft-expected-f32.bin",std::ios::binary);
 require(bool(a)&&bool(b),"Missing independent HRIR reference fixtures");
 a.read(reinterpret_cast<char*>(pressure.data()),pressure.size()*sizeof(double));
 b.read(reinterpret_cast<char*>(expected.data()),expected.size()*sizeof(float));
 require(bool(a)&&bool(b),"Truncated HRIR reference fixture");
 double area[]={2.3e-5,3.1e-5};
 for(int direction=0;direction<3;++direction){
  auto r=sda_hrtf_make_hrir(job,pressure.data()+direction*1024,1024,256,area,1.2,32,output.data(),output.size());
  check_result(r,0,"Native reference/HRIR conversion");
  for(size_t i=0;i<output.size();++i)require(std::abs(output[i]-expected[direction*output.size()+i])<2e-7,"Native HRIR differs from upstream reference/FFT");
 }
 pressure[0]=std::numeric_limits<double>::quiet_NaN();
 auto r=sda_hrtf_make_hrir(job,pressure.data(),1024,256,area,1.2,32,output.data(),output.size());
 check_result(r,1,"Reject non-finite pressure");
 require(std::all_of(output.begin(),output.end(),[](float x){return x==0;}),"Discard partial HRIR on failure");
}
void compare(const std::filesystem::path &root,int ear,int frequency=100){
 auto actual=pressure(root/("NumCalc/source_"+std::to_string(ear)+"/be.out/be."+std::to_string(frequency/100)+"/pEvalGrid"));
 auto golden=pressure(root/("expected/ear"+std::to_string(ear)+"-"+std::to_string(frequency)+"hz.txt"));
 for(size_t i=0;i<actual.size();++i)require(std::abs(actual[i]-golden[i])<=std::max(1e-11,std::abs(golden[i])*1e-6),"Embedded BEM differs from unmodified upstream");
}
int main(int argc,char **argv){
#ifdef __APPLE__
 std::set_terminate([]{void *stack[64];int n=backtrace(stack,64);backtrace_symbols_fd(stack,n,2);std::_Exit(99);});
#endif
try{
 require(argc==2,"Pass a private COPY of the fixture root");
 std::filesystem::path root=std::filesystem::canonical(argv[1]);auto cwd=std::filesystem::current_path();
 uint64_t peak=0;auto j=job(root);
 std::cerr<<"Testing pressure-to-HRIR conversion"<<std::endl;
 impulse_test(root,j.get());
 for(int repeat=0;repeat<2;++repeat)for(int ear=1;ear<=2;++ear){
  std::cerr<<"Testing repeated ear "<<ear<<" pass "<<repeat<<std::endl;
  auto r=sda_hrtf_solver_run(j.get(),("NumCalc/source_"+std::to_string(ear)).c_str(),1,1,0,nullptr,nullptr);
  check_result(r,0,"Repeated ear solve");compare(root,ear);peak=std::max(peak,r.peak_array_bytes);
 }
 for(int frequency:{1000,6000})for(int ear=1;ear<=2;++ear){
  std::cerr<<"Testing FMM frequency "<<frequency<<" ear "<<ear<<std::endl;
  int index=frequency/100;
  auto r=sda_hrtf_solver_run(j.get(),("NumCalc/source_"+std::to_string(ear)).c_str(),index,index,0,nullptr,nullptr);
  check_result(r,0,"Higher-frequency FMM solve");compare(root,ear,frequency);peak=std::max(peak,r.peak_array_bytes);
 }
 std::cerr<<"Testing memory exhaustion and recovery"<<std::endl;
 auto small=job(root,1024*1024);
 check_result(sda_hrtf_solver_run(small.get(),"NumCalc/source_1",1,1,0,nullptr,nullptr),3,"Memory budget");
 check_result(sda_hrtf_solver_run(j.get(),"../",1,1,0,nullptr,nullptr),1,"Work path escape");
 std::filesystem::create_directories(root/"bad-input");
 check_result(sda_hrtf_solver_run(j.get(),"bad-input",1,1,0,nullptr,nullptr),1,"Missing NC.inp returns error without exiting process");
 check_result(sda_hrtf_solver_run(j.get(),"NumCalc/source_1",1,1,1,nullptr,nullptr),0,"RAM estimate after prior failure");
 std::cerr<<"Testing cancellation"<<std::endl;
 auto cancelled=job(root);sda_hrtf_solver_cancel(cancelled.get());
 check_result(sda_hrtf_solver_run(cancelled.get(),"NumCalc/source_1",1,1,0,nullptr,nullptr),2,"Cancellation before run");
 auto during=job(root);
 auto cancel=[](void *p,int,int){sda_hrtf_solver_cancel(static_cast<SdaHrtfSolver*>(p));};
 check_result(sda_hrtf_solver_run(during.get(),"NumCalc/source_1",1,1,0,cancel,during.get()),2,"Cancellation from progress");
 // Actual concurrent solves to disjoint ear files in the same process: private
 // native state must prevent left/right contamination without changing cwd.
 std::cerr<<"Testing concurrent native jobs"<<std::endl;
 auto left=job(root),right=job(root);
 auto future=std::async(std::launch::async,[&]{return sda_hrtf_solver_run(left.get(),"NumCalc/source_1",1,1,0,nullptr,nullptr);});
 check_result(sda_hrtf_solver_run(right.get(),"NumCalc/source_2",1,1,0,nullptr,nullptr),0,"Concurrent right solve");
 check_result(future.get(),0,"Concurrent left solve");compare(root,1);compare(root,2);
 require(std::filesystem::current_path()==cwd,"Native library changed process cwd");
 std::cout<<"{\"passed\":true,\"upstreamPressureMatch\":true,\"upstreamImpulseMatch\":true,\"repeatedEars\":4,\"concurrentJobs\":2,\"memoryLimit\":true,\"cancellation\":true,\"errorRecovery\":true,\"cwdUnchanged\":true,\"peakTrackedArrayBytes\":"<<peak<<",\"physicalScanTested\":false}"<<std::endl;
 return 0;
}catch(const std::exception &e){std::cerr<<e.what()<<std::endl;return 1;}}
