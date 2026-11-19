#include "SdaHrtfSolver.h"
#include <filesystem>
#include <fstream>
#include <vector>
#include <sstream>
#include <iomanip>
#include <iostream>
#include <stdexcept>
#include <algorithm>
#include <memory>
#include <cmath>
namespace fs=std::filesystem;
static void require(bool test,const char *text){if(!test)throw std::runtime_error(text);}
static void write(const fs::path &file,const std::string &text){fs::create_directories(file.parent_path());std::ofstream stream(file,std::ios::binary);stream<<text;require(bool(stream),"Cannot write fixture");}
static std::string read(const fs::path &file){std::ifstream stream(file,std::ios::binary);return {std::istreambuf_iterator<char>(stream),{}};}
// A deliberately coarse, 4 cm octahedron verifies REAL NumCalc file-format
// compatibility, not head accuracy. No fixture from this test is installable.
static void generated_solve(const fs::path &root) {
 constexpr int bins=16,count=34;const double radius=.02,pi=3.14159265358979323846;
 std::ostringstream nodes,elements,grid,gridElements;
 nodes<<6<<"\n"<<"0 .02 0 0\n1 0 .02 0\n2 -.02 0 0\n3 0 -.02 0\n4 0 0 .02\n5 0 0 -.02\n";
 elements<<8<<"\n";
 for(int i=0;i<4;++i){elements<<i<<" "<<i<<" "<<(i+1)%4<<" 4 0 0 0\n";elements<<i+4<<" "<<(i+1)%4<<" "<<i<<" 5 0 0 0\n";}
 grid<<std::setprecision(17)<<count<<"\n";
 for(int i=0;i<32;++i)grid<<400000+i<<" "<<1.2*std::cos(i*2*pi/32)<<" "<<1.2*std::sin(i*2*pi/32)<<" 0\n";
 grid<<"400032 0 0 -1.2\n400033 0 0 1.2\n";
 gridElements<<64<<"\n";
 for(int i=0;i<32;++i){gridElements<<400000+i<<" "<<400000+i<<" "<<400000+(i+1)%32<<" 400033 2 0 1\n";gridElements<<400032+i<<" "<<400000+(i+1)%32<<" "<<400000+i<<" 400032 2 0 1\n";}
 write(root/"ObjectMeshes/Reference/Nodes.txt",nodes.str());write(root/"ObjectMeshes/Reference/Elements.txt",elements.str());
 write(root/"EvaluationGrids/FullSphere/Nodes.txt",grid.str());write(root/"EvaluationGrids/FullSphere/Elements.txt",gridElements.str());
 for(int ear=1;ear<=2;++ear){
  std::ostringstream nc;nc<<std::setprecision(17)<<"Mesh2HRTF 1.0.0\nSynthetic octahedron, NOT a personal head\n0 0 0 0 7 0\n1 16 .000001 0 1 0 0\n0 17\n";
  for(int k=0;k<=bins;++k)nc<<double(k)*1e-6<<" "<<double(k)*24000/bins<<" 0\n";
  nc<<"2 72 40 0 0 2 1 4 0\n0 0 0 0 0 0 0\n0 0 0 0\n343 1.1839 1 0 0 0 0\nNODES\n../../ObjectMeshes/Reference/Nodes.txt\n../../EvaluationGrids/FullSphere/Nodes.txt\nELEMENTS\n../../ObjectMeshes/Reference/Elements.txt\n../../EvaluationGrids/FullSphere/Elements.txt\nBOUNDARY\nELEM "<<(ear==1?0:6)<<" TO "<<(ear==1?0:6)<<" VELO .1 -1 0 -1\nRETU\nPOST PROCESS\nEND\n";
  write(root/("NumCalc/source_"+std::to_string(ear)+"/NC.inp"),nc.str());
 }
 std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)> job(sda_hrtf_solver_create(root.string().c_str(),32*1024*1024),sda_hrtf_solver_destroy);require(bool(job),"Cannot create generated numerical test");
 for(int ear=1;ear<=2;++ear){auto reply=sda_hrtf_solver_run(job.get(),("NumCalc/source_"+std::to_string(ear)).c_str(),1,bins,0,nullptr,nullptr);require(reply.code==0,reply.message);}
 int32_t ids[count];for(int i=0;i<count;++i)ids[i]=400000+i;std::vector<double> bank(count*4*bins);
 auto reply=sda_hrtf_read_pressure(job.get(),ids,count,bins,bank.data(),bank.size());require(reply.code==0,reply.message);
 double areas[]={std::sqrt(3.)*radius*radius/2,std::sqrt(3.)*radius*radius/2};std::vector<float> impulse(4*bins);
 for(int d=0;d<count;++d){auto result=sda_hrtf_make_hrir(job.get(),bank.data()+d*4*bins,4*bins,bins,areas,1.2,8,impulse.data(),impulse.size());require(result.code==0,result.message);}
}
int main(int argc,char **argv)try {
 require(argc==3,"Pass FFT fixture and temporary output directory");const fs::path fixture=fs::canonical(argv[1]),root=fs::absolute(argv[2]);
 fs::create_directories(root);constexpr size_t bins=256,directions=3;
 std::vector<double> expected(directions*4*bins);std::ifstream input(fixture/"fft-pressure-f64.bin",std::ios::binary);input.read(reinterpret_cast<char*>(expected.data()),expected.size()*sizeof(double));require(bool(input),"Missing FFT reference pressures");
 std::ifstream irInput(fixture/"fft-expected-f32.bin",std::ios::binary);std::vector<float> irExpected(directions*4*bins);irInput.read(reinterpret_cast<char*>(irExpected.data()),irExpected.size()*sizeof(float));require(bool(irInput),"Missing upstream HRIR reference");
 std::ostringstream curve;curve<<std::setprecision(17)<<"Mesh2HRTF 1.0.0\nSynthetic result-reader fixture only\n0 0 0 0 7 0\n1 256 0.000001 0 1 0 0\n0 257\n";
 for(size_t k=0;k<=bins;++k)curve<<double(k)*1e-6<<" "<<double(k)*24000.0/bins<<" 0\n";
 for(int ear=1;ear<=2;++ear){
  write(root/("NumCalc/source_"+std::to_string(ear)+"/NC.inp"),curve.str());
  for(size_t k=0;k<bins;++k){std::ostringstream rows;rows<<std::setprecision(17)<<"Mesh2HRTF 1.0.0\n1\n1 3\n";
   // Deliberately reverse node order: match stable IDs, never row position.
   for(int d=2;d>=0;--d){auto offset=(size_t(d)*2*bins+size_t(ear-1)*bins+k)*2;rows<<300000+d<<" "<<expected[offset]<<" "<<expected[offset+1]<<"\n";}
   write(root/("NumCalc/source_"+std::to_string(ear)+"/be.out/be."+std::to_string(k+1)+"/pEvalGrid"),rows.str());
  }
 }
 std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)> job(sda_hrtf_solver_create(root.string().c_str(),8*1024*1024),sda_hrtf_solver_destroy);
 require(bool(job),"Cannot create reader job");int32_t ids[]={300000,300001,300002};std::vector<double> output(expected.size(),9);
 auto run=[&](){return sda_hrtf_read_pressure(job.get(),ids,directions,bins,output.data(),output.size());};
 auto result=run();require(result.code==0,result.message);require(output==expected,"Evaluation reader changed pressure or channel/direction order");
 double areas[]={2.3e-5,3.1e-5};std::vector<float> ir(4*bins);
 for(size_t d=0;d<directions;++d){auto converted=sda_hrtf_make_hrir(job.get(),output.data()+d*4*bins,4*bins,bins,areas,1.2,32,ir.data(),ir.size());require(converted.code==0,converted.message);for(size_t k=0;k<ir.size();++k)require(std::abs(ir[k]-irExpected[d*ir.size()+k])<2e-6,"Reader-to-HRIR differs from upstream reference");}
 auto reject=[&](){std::fill(output.begin(),output.end(),9);require(run().code!=0,"Invalid result accepted");require(std::all_of(output.begin(),output.end(),[](double v){return v==0;}),"Partial failure output was retained");};
 const auto pressure=root/"NumCalc/source_2/be.out/be.256/pEvalGrid";const auto original=read(pressure);
 write(pressure,"Mesh2HRTF 1.0.0\n1\n1 3\n300000 1 0\n300000 2 0\n300002 3 0\n");reject();
 write(pressure,"Mesh2HRTF 1.0.0\n1\n1 2\n300000 1 0\n300001 2 0\n");reject();
 write(pressure,"Mesh2HRTF 1.0.0\n1\n1 3\n300000 1 0\n300001 nan 0\n300002 3 0\n");reject();
 write(pressure,original+"unexpected\n");reject();fs::remove(pressure);reject();write(pressure,original);
 const auto nc=root/"NumCalc/source_1/NC.inp";auto bad=curve.str();auto at=bad.find("24000 0");require(at!=std::string::npos,"No final frequency");bad.replace(at,7,"23000 0");write(nc,bad);reject();
 bad=curve.str();at=bad.find("1 256 0.000001");bad.replace(at,14,"1 256 0.000002");write(nc,bad);reject();write(nc,curve.str());
 ids[2]=300000;reject();ids[2]=300002;require(run().code==0,"Reader did not recover after invalid result");
 std::vector<std::string> saved;
 for(size_t k=1;k<=bins;++k){auto file=root/("NumCalc/source_2/be.out/be."+std::to_string(k)+"/pEvalGrid");saved.push_back(read(file));write(file,"Mesh2HRTF 1.0.0\n1\n1 3\n300000 0 0\n300001 0 0\n300002 0 0\n");}
 reject();
 for(size_t k=1;k<=bins;++k)write(root/("NumCalc/source_2/be.out/be."+std::to_string(k)+"/pEvalGrid"),saved[k-1]);
 require(run().code==0,"Reader did not recover after all-zero ear rejection");
 sda_hrtf_solver_cancel(job.get());reject();
 generated_solve(root/"actual-numcalc-format-check");
 std::cout<<"{\"passed\":true,\"upstreamImpulseMatch\":true,\"actualNumCalcResultRead\":true,\"nodeOrderChecked\":true,\"frequencyGridChecked\":true,\"missingAndDuplicateRejected\":true,\"nonfiniteRejected\":true,\"zeroEarRejected\":true,\"cancellation\":true,\"physicalScanTested\":false}"<<std::endl;
 return 0;
}catch(const std::exception &e){std::cerr<<e.what()<<std::endl;return 1;}
