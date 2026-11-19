// On-device reference pressure / HRIR conversion, equivalent to the
// Mesh2HRTF v1.3.0 EUPL-1.2 Output2HRTF equations (see NOTICE.txt).
#include "EmbeddedRuntime.hpp"
namespace {
constexpr double pi=3.1415926535897932384626433832795;
void inverse_fft(std::vector<std::complex<double>> &a,SdaHrtfSolver *job) {
 const size_t n=a.size();
 for(size_t i=1,j=0;i<n;++i){
  size_t bit=n>>1;for(;j&bit;bit>>=1)j^=bit;j^=bit;
  if(i<j)std::swap(a[i],a[j]);
 }
 for(size_t length=2;length<=n;length<<=1){
  if(job->cancelled.load(std::memory_order_relaxed))throw sda_numcalc::Failure(2,"Calculation cancelled");
  auto step=std::polar(1.0,2*pi/double(length));
  for(size_t start=0;start<n;start+=length){
   std::complex<double> w(1,0);
   for(size_t j=0;j<length/2;++j){
    auto u=a[start+j],v=w*a[start+j+length/2];
    a[start+j]=u+v;a[start+j+length/2]=u-v;w*=step;
   }
  }
 }
 for(auto &value:a)value/=double(n);
}
}
extern "C" SdaHrtfSolverResult sda_hrtf_make_hrir(SdaHrtfSolver *job,const double *pressure,size_t values,size_t bins,const double *areas,double radius,int shift,float *output,size_t samples){
 SdaHrtfSolverResult reply{};
 auto error=[&](int code,const char *s){reply.code=code;std::snprintf(reply.message,sizeof(reply.message),"%s",s);};
 if(!job || !pressure || !areas || !output || bins<16 || bins>2048 || (bins&(bins-1)) || values!=4*bins || samples!=4*bins || shift<0 || size_t(shift)>=2*bins || !std::isfinite(radius) || radius<0.25 || radius>10){error(1,"Invalid local HRIR dimensions or reference radius");return reply;}
 for(int ear=0;ear<2;++ear)if(!std::isfinite(areas[ear]) || areas[ear]<1e-9 || areas[ear]>.001){error(1,"Invalid measured ear source area");return reply;}
 // Zero on every failure, so partially computed samples cannot be installed.
 std::fill(output,output+samples,0);
 if(sda_numcalc::runtime){error(1,"HRIR conversion cannot re-enter a solver callback");return reply;}
 std::lock_guard<std::mutex> lock(job->mutex);
 try{
  if(job->cancelled.load(std::memory_order_relaxed))throw sda_numcalc::Failure(2,"Calculation cancelled");
  const size_t n=2*bins;
  const uint64_t bytes=n*sizeof(std::complex<double>);
  if(bytes>job->budget)throw sda_numcalc::Failure(3,"Local HRIR conversion exceeds memory budget");
  reply.peak_array_bytes=bytes;
  for(size_t ear=0;ear<2;++ear){
   std::vector<std::complex<double>> spectrum(n);
   spectrum[0]=1.0; // defined low-frequency free-field reference
   for(size_t k=1;k<=bins;++k){
    const size_t offset=2*(ear*bins+k-1);
    if(!std::isfinite(pressure[offset]) || !std::isfinite(pressure[offset+1]))throw sda_numcalc::Failure(1,"Non-finite acoustic pressure");
    const double frequency=double(k)*48000.0/double(n);
    // Upstream reciprocity source: 0.1 m/s on the measured ear triangle.
    const std::complex<double> freeField=std::complex<double>(0,-1)*1.1839*2*pi*frequency*(.1*areas[ear])/(4*pi)*std::polar(1.0,2*pi*frequency/343.0*radius)/radius;
    const auto h=std::complex<double>(pressure[offset],pressure[offset+1])/freeField;
    if(!std::isfinite(h.real()) || !std::isfinite(h.imag()))throw sda_numcalc::Failure(1,"Invalid referenced transfer function");
    if(k==bins)spectrum[k]=std::abs(h);
    else {spectrum[k]=std::conj(h);spectrum[n-k]=h;}
   }
   inverse_fft(spectrum,job);
   for(size_t k=0;k<n;++k){
    double sample=spectrum[k].real();
    if(!std::isfinite(sample) || std::abs(sample)>100)throw sda_numcalc::Failure(1,"Invalid local ear impulse");
    output[ear*n+(k+size_t(shift))%n]=float(sample);
   }
  }
 }catch(const sda_numcalc::Failure &e){error(e.code,e.what());}
 catch(const std::bad_alloc &){error(3,"Insufficient memory for local HRIR conversion");}
 catch(const std::exception &e){error(1,e.what());}
 catch(...){error(1,"Local HRIR conversion failed");}
 if(reply.code)std::fill(output,output+samples,0);
 return reply;
}
