// Local NumCalc evaluation reader; no Python, shell, network or cwd changes.
#include "EmbeddedRuntime.hpp"
#include <locale>
#include <unordered_set>
namespace {
using sda_numcalc::Failure;
void check(SdaHrtfSolver *job) {
 if(job->cancelled.load(std::memory_order_relaxed))throw Failure(2,"Result processing cancelled");
 const uint64_t available=sda_hrtf_available_memory();
 if(available && available<128ull*1024*1024)throw Failure(3,"Insufficient iPhone memory headroom for results");
}
bool within(const std::filesystem::path &root,const std::filesystem::path &path) {
 auto p=path.begin();for(auto r=root.begin();r!=root.end();++r,++p)if(p==path.end() || *r!=*p)return false;
 return true;
}
std::string read_file(SdaHrtfSolver *job,const std::string &relative) {
 check(job);
 auto path=std::filesystem::canonical(job->root/relative);
 if(!within(job->root,path) || !std::filesystem::is_regular_file(path))throw Failure(1,"Result escapes private job directory");
 const auto size=std::filesystem::file_size(path);
 if(size==0 || size>1024*1024)throw Failure(1,"Invalid acoustic result file size");
 std::ifstream file(path,std::ios::binary);std::string data(size,'\0');
 if(!file.read(data.data(),std::streamsize(size)) || file.peek()!=std::char_traits<char>::eof())throw Failure(1,"Cannot read complete acoustic result");
 return data;
}
std::vector<std::string> records(const std::string &data) {
 std::istringstream file(data);std::vector<std::string> lines;std::string line;
 while(std::getline(file,line)) {
  if(line.size()>4096)throw Failure(1,"Oversized acoustic input record");
  auto first=line.find_first_not_of(" \t\r");
  if(first!=std::string::npos && line[first]!='#') {
   if(lines.size()>=8192)throw Failure(1,"Too many acoustic input records");
   lines.push_back(line.substr(first));
  }
 }
 return lines;
}
// Strictly accept the embedded project's constant-frequency-spacing format.
// This prevents treating a log-spaced or incomplete solve as an FFT spectrum.
void validate_frequencies(SdaHrtfSolver *job,int ear,size_t bins) {
 auto lines=records(read_file(job,"NumCalc/source_"+std::to_string(ear)+"/NC.inp"));
 if(lines.size()<6+bins)throw Failure(1,"Incomplete acoustic frequency curve");
 std::istringstream count(lines[3]);count.imbue(std::locale::classic());
 int method=0,steps=0;double step=0,start=0;if(!(count>>method>>steps>>step>>start) || method!=1 || steps!=int(bins) || !std::isfinite(step) || step<=0 || start!=0)throw Failure(1,"Acoustic frequency count mismatch");
 std::istringstream curve(lines[4]);curve.imbue(std::locale::classic());int id=-1,points=0;
 if(!(curve>>id>>points) || id!=0 || points!=int(bins+1))throw Failure(1,"Invalid acoustic frequency curve");
 double lastTime=-1;
 for(size_t k=0;k<=bins;++k) {
  std::istringstream row(lines[5+k]);row.imbue(std::locale::classic());double time=0,hz=0,imag=0;
  if(!(row>>time>>hz>>imag) || !std::isfinite(time) || !std::isfinite(hz) || !std::isfinite(imag) || time<=lastTime || std::abs(time-double(k)*step)>std::max(1e-12,std::abs(time)*1e-8) || imag!=0 || std::abs(hz-double(k)*24000.0/double(bins))>1e-7)
   throw Failure(1,"Acoustic frequencies do not match 48 kHz FFT bins");
  row>>std::ws;if(!row.eof())throw Failure(1,"Unexpected acoustic frequency data");lastTime=time;
 }
}
}
extern "C" SdaHrtfSolverResult sda_hrtf_read_pressure(SdaHrtfSolver *job,const int32_t *ids,size_t directions,size_t bins,double *output,size_t values) {
 SdaHrtfSolverResult reply{};
 auto error=[&](int code,const char *text){reply.code=code;std::snprintf(reply.message,sizeof(reply.message),"%s",text);};
 if(!job || !ids || !output || directions<1 || directions>2048 || bins<16 || bins>2048 || (bins&(bins-1)) || values!=directions*4*bins) {
  error(1,"Invalid local pressure dimensions");return reply;
 }
 std::fill(output,output+values,0);
 if(values>64ull*1024*1024/sizeof(double) || job->budget<4*1024*1024 || values>(job->budget-4*1024*1024)/sizeof(double)) { error(3,"Pressure bank exceeds local memory budget");return reply; }
 if(sda_numcalc::runtime){error(1,"Cannot read pressure inside a solver callback");return reply;}
 std::lock_guard<std::mutex> guard(job->mutex);
 try {
  check(job);std::unordered_map<int32_t,size_t> index;
  for(size_t d=0;d<directions;++d)if(ids[d]<0 || !index.emplace(ids[d],d).second)throw Failure(1,"Duplicate or invalid evaluation ID");
  for(int ear=1;ear<=2;++ear) {
   validate_frequencies(job,ear,bins);
   for(size_t k=0;k<bins;++k) {
    auto data=read_file(job,"NumCalc/source_"+std::to_string(ear)+"/be.out/be."+std::to_string(k+1)+"/pEvalGrid");
    std::istringstream file(data);file.imbue(std::locale::classic());std::string header;
    if(!std::getline(file,header) || (header!="Mesh2HRTF 1.0.0" && header!="Mesh2HRTF 1.0.0\r"))throw Failure(1,"Unknown acoustic pressure format");
    int groups=0,group=0;size_t count=0;
    if(!(file>>groups>>group>>count) || groups!=1 || group!=1 || count!=directions)throw Failure(1,"Acoustic evaluation count mismatch");
    std::unordered_set<int32_t> seen;
    for(size_t row=0;row<directions;++row) {
     int32_t id=0;double real=0,imag=0;
     if(!(file>>id>>real>>imag) || !std::isfinite(real) || !std::isfinite(imag))throw Failure(1,"Invalid acoustic pressure sample");
     const auto found=index.find(id);
     if(found==index.end() || !seen.insert(id).second)throw Failure(1,"Missing, duplicate or unknown evaluation ID");
     const size_t offset=(found->second*2*bins+size_t(ear-1)*bins+k)*2;
     output[offset]=real;output[offset+1]=imag;
    }
    file>>std::ws;if(!file.eof())throw Failure(1,"Extra acoustic pressure data");
   }
  }
  // Caller-owned bank plus bounded per-file parsing and evaluation index.
  for(size_t d=0;d<directions;++d) {
   check(job);
   for(size_t ear=0;ear<2;++ear) {
    bool nonzero=false;
    for(size_t k=0;k<2*bins;++k)nonzero=nonzero || output[(d*2+ear)*2*bins+k]!=0;
    if(!nonzero)throw Failure(1,"Acoustic direction has an entirely zero ear spectrum");
   }
  }
  reply.peak_array_bytes=values*sizeof(double)+4*1024*1024;
 }catch(const Failure &e){error(e.code,e.what());}
 catch(const std::bad_alloc &){error(3,"Insufficient memory for acoustic results");}
 catch(const std::exception &){error(1,"Missing or unreadable acoustic result");}
 catch(...){error(1,"Local pressure reading failed");}
 if(reply.code)std::fill(output,output+values,0);
 return reply;
}
