// Procedural front/side ellipsoid, no rear points and no claim of human pinnae.
#include "SdaHrtfProject.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>
namespace fs=std::filesystem;
static void expect(bool p,const char* s){if(!p)throw std::runtime_error(s);}
struct Shell {
 std::vector<float> xyz,cloud,normals;std::vector<uint32_t> tris;uint32_t ears[2]{};
 static constexpr double pi=3.141592653589793;
 Shell(){constexpr int lat=96,az=160;const double extent=2.16;
  auto vertex=[&](double t,double a){xyz.insert(xyz.end(),{float(.095*std::cos(t)),float(.078*std::sin(t)*std::cos(a)),float(.101*std::sin(t)*std::sin(a))});};
  vertex(0,0);for(int j=1;j<=lat;++j)for(int k=0;k<az;++k)vertex(extent*j/lat,2*pi*k/az);
  auto id=[=](int j,int k){return uint32_t(1+(j-1)*az+k%az);};auto tri=[&](uint32_t a,uint32_t b,uint32_t c){tris.insert(tris.end(),{a,b,c});};
  for(int k=0;k<az;++k)tri(0,id(1,k),id(1,k+1));
  for(int j=1;j<lat;++j)for(int k=0;k<az;++k){if(j==69 && (k==0 || k==az/2))ears[k==0?0:1]=uint32_t(tris.size()/3);tri(id(j,k),id(j+1,k),id(j+1,k+1));tri(id(j,k),id(j+1,k+1),id(j,k+1));}
  // Independent angular offsets, denser than review geometry; no samples for the baffle.
  for(int j=0;j<=230;++j)for(int k=0;k<512;++k){double t=extent*(j+.12)/230.12,a=2*pi*(k+.37)/512.,x=std::cos(t),y=std::sin(t)*std::cos(a),z=std::sin(t)*std::sin(a);
   cloud.insert(cloud.end(),{float(.095*x),float(.078*y),float(.101*z)});double n=std::hypot(x/.095,y/.078,z/.101);normals.insert(normals.end(),{float(x/(.095*n)),float(y/(.078*n)),float(z/(.101*n))});}
 }
 SdaHrtfProjectInput view(){SdaHrtfProjectInput v{};v.mesh={xyz.data(),xyz.size()/3,tris.data(),tris.size()/3,cloud.data(),normals.data(),cloud.size()/3};v.left_faces=ears;v.right_faces=ears+1;v.left_face_count=v.right_face_count=1;v.front[0]=v.up[2]=1;return v;}
 void omit(std::set<uint32_t> missing){
  for(auto &ear:ears){expect(!missing.count(ear),"Fixture removed a confirmed source");ear-=uint32_t(std::distance(missing.begin(),missing.lower_bound(ear)));}
  std::vector<uint32_t> kept;for(uint32_t f=0;f<tris.size()/3;++f)if(!missing.count(f))kept.insert(kept.end(),tris.begin()+3*f,tris.begin()+3*f+3);tris.swap(kept);
 }
};
int main(int argc,char** argv)try{
 expect(argc==2,"Pass fresh test root");fs::path root=fs::absolute(argv[1]);expect(!fs::exists(root),"Fresh directory required");fs::create_directories(root);Shell s;
 auto run=[&](const char* job,SdaHrtfProjectInput v,SdaHrtfMeshCancelled c=nullptr,void *context=nullptr){auto p=root/job;fs::create_directory(p);auto r=sda_hrtf_prepare_review_project(p.string().c_str(),v,1536ull<<20,4ull<<30,c,context);if(r.project.code)expect(fs::is_empty(p),"Rejected input published partial project");return r;};
 auto ok=run("ok",s.view());if(ok.project.code)throw std::runtime_error(ok.project.message);
 expect(ok.boundary_faces && ok.measured_faces<s.tris.size()/3 && ok.measured_faces+ok.boundary_faces<20000 && ok.review_deviation<=.0012 && ok.project.directions==114,"Invalid reviewed mesh provenance or capacity");
 auto sourceArea=[&](uint32_t id){auto *p=s.tris.data()+3*id;std::array<double,3>a{},b{},n{};for(int i=0;i<3;++i){a[i]=double(s.xyz[3*p[1]+i])-s.xyz[3*p[0]+i];b[i]=double(s.xyz[3*p[2]+i])-s.xyz[3*p[0]+i];}for(int i=0;i<3;++i)n[i]=a[(i+1)%3]*b[(i+2)%3]-a[(i+2)%3]*b[(i+1)%3];return .5*std::hypot(n[0],n[1],n[2]);};
 for(int ear=0;ear<2;++ear)expect(std::abs(ok.project.source_areas[ear]-sourceArea(s.ears[ear]))<1e-14,"Source face area changed");
 auto v=s.view();std::swap(v.left_faces,v.right_faces);expect(run("swapped",v).project.code==1,"Inverted ears accepted");
 auto checkGap=[&](const SdaHrtfReviewProjectResult &r,const char *why,const char *where){expect(r.project.code==1 && r.boundary_issue && std::string(r.boundary_issue)==why && r.boundary_region && std::string(r.boundary_region)==where,"Wrong boundary failure classification");};
 auto broken=s;broken.omit({4});auto repaired=run("anterior-with-depth",broken.view());if(repaired.project.code)throw std::runtime_error(repaired.project.message);
 expect(repaired.retriangulated_faces==1 && repaired.review_loops==2 && repaired.boundary_faces>0,"Depth-backed face was not distinguished from posterior baffle");
 for(int e=0;e<2;++e)expect(std::abs(repaired.project.source_areas[e]-ok.project.source_areas[e])<1e-14,"Repair changed confirmed ears");
 auto cell=[](int j,int k){return uint32_t(160+2*((j-1)*160+k));};
 auto several=s;auto q=cell(24,40);several.omit({4,q,q+1});auto multi=run("two-supported-holes",several.view());if(multi.project.code)throw std::runtime_error(multi.project.message);
 expect(multi.review_loops==3 && multi.retriangulated_faces==3 && multi.boundary_faces>0,"Multiple supported holes were not recovered");
 // Remove independent depth from a neighbourhood, but leave perfectly aligned
 // samples along all three edges. Ordinary nearest-depth checks alone pass here.
 for(bool flipped:{false,true}){
  auto gap=broken;gap.cloud.clear();gap.normals.clear();
  for(size_t i=0;i<s.cloud.size();i+=3){bool near=s.cloud[i]>.09495f;
   if(near && !flipped)continue;for(int j=0;j<3;++j){gap.cloud.push_back(s.cloud[i+j]);gap.normals.push_back((near?-1:1)*s.normals[i+j]);}}
  auto t=s.tris.data()+12;for(int k=0;k<3;++k)for(int h=0;h<=12;++h){for(int j=0;j<3;++j){gap.cloud.push_back(float((12-h)*double(s.xyz[3*t[k]+j])/12+h*double(s.xyz[3*t[(k+1)%3]+j])/12));gap.normals.push_back(j==0?1.f:0.f);}}
  checkGap(run(flipped?"opposite-interior-normals":"rim-only",gap.view()),"insufficient-interior-depth","front-or-side");
 }
 for(int e=0;e<2;++e){auto gap=s;gap.omit({s.ears[e]+1});checkGap(run(e?"right-source-gap":"left-source-gap",gap.view()),"confirmed-source-gap",e?"right-ear":"left-ear");}
 auto ambiguous=s;std::set<uint32_t> strip;for(int k=16;k<21;++k){auto a=cell(93,k);strip.insert(a);strip.insert(a+1);}ambiguous.omit(strip);
 checkGap(run("ambiguous-posterior",ambiguous.view()),"ambiguous-posterior-contours","posterior");
 v=s.view();v.mesh.support_count=128;expect(run("no-depth",v).project.code==1,"Unsupported measured faces accepted");
 expect(run("cancel",s.view(),[](void*){return 1;}).project.code==2,"Review cancellation ignored");
 int calls=0;expect(run("cancel-during-repair",broken.view(),[](void *ctx){return int(++*static_cast<int*>(ctx)>=900);},&calls).project.code==2 && calls>=900,"Mid-work repair cancellation ignored");
 std::cout<<"{\"passed\":true,\"inputFaces\":"<<s.tris.size()/3<<",\"measuredFaces\":"<<ok.measured_faces<<",\"boundaryFaces\":"<<ok.boundary_faces<<",\"reviewDeviationM\":"<<ok.review_deviation<<",\"verifiedLocalTriangles\":"<<multi.retriangulated_faces<<",\"rimOnlyRejected\":true,\"wrongNormalsRejected\":true,\"physicalScan\":false}\n";
}catch(const std::exception &e){std::cerr<<e.what()<<'\n';return 1;}
