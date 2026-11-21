// Procedural front/side ellipsoid, no rear points and no claim of human pinnae.
#include "SdaHrtfProject.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
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
};
int main(int argc,char** argv)try{
 expect(argc==2,"Pass fresh test root");fs::path root=fs::absolute(argv[1]);expect(!fs::exists(root),"Fresh directory required");fs::create_directories(root);Shell s;
 auto run=[&](const char* job,SdaHrtfProjectInput v,SdaHrtfMeshCancelled c=nullptr){auto p=root/job;fs::create_directory(p);auto r=sda_hrtf_prepare_review_project(p.string().c_str(),v,1536ull<<20,4ull<<30,c,nullptr);if(r.project.code)expect(fs::is_empty(p),"Rejected input published partial project");return r;};
 auto ok=run("ok",s.view());if(ok.project.code)throw std::runtime_error(ok.project.message);
 expect(ok.boundary_faces && ok.measured_faces<s.tris.size()/3 && ok.measured_faces+ok.boundary_faces<20000 && ok.review_deviation<=.0012 && ok.project.directions==114,"Invalid reviewed mesh provenance or capacity");
 auto sourceArea=[&](uint32_t id){auto *p=s.tris.data()+3*id;std::array<double,3>a{},b{},n{};for(int i=0;i<3;++i){a[i]=double(s.xyz[3*p[1]+i])-s.xyz[3*p[0]+i];b[i]=double(s.xyz[3*p[2]+i])-s.xyz[3*p[0]+i];}for(int i=0;i<3;++i)n[i]=a[(i+1)%3]*b[(i+2)%3]-a[(i+2)%3]*b[(i+1)%3];return .5*std::hypot(n[0],n[1],n[2]);};
 for(int ear=0;ear<2;++ear)expect(std::abs(ok.project.source_areas[ear]-sourceArea(s.ears[ear]))<1e-14,"Source face area changed");
 auto v=s.view();std::swap(v.left_faces,v.right_faces);expect(run("swapped",v).project.code==1,"Inverted ears accepted");
 auto broken=s.tris;broken.erase(broken.begin()+12,broken.begin()+15);v=s.view();v.mesh.triangles=broken.data();--v.mesh.triangle_count;uint32_t ears[]={s.ears[0]-1,s.ears[1]-1};v.left_faces=ears;v.right_faces=ears+1;expect(run("anterior-hole",v).project.code==1,"Anterior gap treated as posterior");
 v=s.view();v.mesh.support_count=128;expect(run("no-depth",v).project.code==1,"Unsupported measured faces accepted");
 expect(run("cancel",s.view(),[](void*){return 1;}).project.code==2,"Review cancellation ignored");
 std::cout<<"{\"passed\":true,\"inputFaces\":"<<s.tris.size()/3<<",\"measuredFaces\":"<<ok.measured_faces<<",\"boundaryFaces\":"<<ok.boundary_faces<<",\"reviewDeviationM\":"<<ok.review_deviation<<",\"physicalScan\":false}\n";
}catch(const std::exception &e){std::cerr<<e.what()<<'\n';return 1;}
