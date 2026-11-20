// Synthetic geometry regression only. These are NOT human scans or HRTFs.
#include "SdaHrtfMesh.h"
#include <array>
#include <cmath>
#include <iostream>
#include <map>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>
struct Fixture {
 std::vector<float> vertices,support,normals;std::vector<uint32_t> triangles;
 SdaHrtfMeshView view(){return {vertices.data(),vertices.size()/3,triangles.data(),triangles.size()/3,support.data(),normals.data(),support.size()/3};}
};
void require(bool ok,const char *message){if(!ok)throw std::runtime_error(message);}
Fixture cube(int n=8){
 Fixture f;std::map<std::array<int,3>,uint32_t> ids;
 auto vertex=[&](std::array<int,3> p){auto result=ids.emplace(p,uint32_t(ids.size()));if(result.second)for(int v:p)f.vertices.push_back(float(v)*(0.02f/n)-0.01f);return result.first->second;};
 for(int axis=0;axis<3;++axis)for(int side:{0,n})for(int i=0;i<n;++i)for(int j=0;j<n;++j){
  auto id=[&](int u,int v){std::array<int,3> p{};p[axis]=side;p[(axis+1)%3]=u;p[(axis+2)%3]=v;return vertex(p);};
  uint32_t a=id(i,j),b=id(i+1,j),c=id(i+1,j+1),d=id(i,j+1);
  if(side==n)f.triangles.insert(f.triangles.end(),{a,b,c,a,c,d});else f.triangles.insert(f.triangles.end(),{a,c,b,a,d,c});
 }
 for(size_t i=0;i<f.triangles.size();i+=3){
  std::array<std::array<double,3>,3> v{};for(int k=0;k<3;++k)for(int j=0;j<3;++j)v[k][j]=f.vertices[3*f.triangles[i+k]+j];
  std::array<double,3> normal{};
  for(int j=0;j<3;++j)normal[j]=(v[1][(j+1)%3]-v[0][(j+1)%3])*(v[2][(j+2)%3]-v[0][(j+2)%3])-(v[1][(j+2)%3]-v[0][(j+2)%3])*(v[2][(j+1)%3]-v[0][(j+1)%3]);
  double length=std::sqrt(normal[0]*normal[0]+normal[1]*normal[1]+normal[2]*normal[2]);
  if(n>=64){
   for(int j=0;j<3;j++){f.support.push_back(float((v[0][j]+v[1][j]+v[2][j])/3));f.normals.push_back(float(normal[j]/length));}
   continue;
  }
  for(int a=0;a<=3;++a)for(int b=0;b<=3-a;++b)for(int j=0;j<3;++j){
   f.support.push_back(float(v[0][j]+(v[1][j]-v[0][j])*a/3+(v[2][j]-v[0][j])*b/3));f.normals.push_back(float(normal[j]/length));
  }
 }
 return f;
}
void check(Fixture &f,int code,const char *name,const char *message=nullptr){auto r=sda_hrtf_screen_head_mesh(f.view(),nullptr,nullptr);if(r.code!=code||(message&&std::string(r.message).find(message)==std::string::npos)){std::cerr<<name<<": "<<r.code<<" "<<r.message<<std::endl;throw std::runtime_error(name);}}
Fixture selected(Fixture &input, const std::vector<uint8_t> &mask){
 Fixture out;out.support=input.support;out.normals=input.normals;
 std::vector<uint32_t> remap(input.vertices.size()/3,UINT32_MAX);
 for(size_t f=0;f<mask.size();++f)if(mask[f])for(int j=0;j<3;++j){
  uint32_t old=input.triangles[3*f+j];
  if(remap[old]==UINT32_MAX){remap[old]=uint32_t(out.vertices.size()/3);out.vertices.insert(out.vertices.end(),input.vertices.begin()+3*old,input.vertices.begin()+3*old+3);}
  out.triangles.push_back(remap[old]);
 }
 return out;
}
void measurementTests(Fixture &good){
 auto noRear=good;noRear.support.clear();noRear.normals.clear();
 // Remove ALL measurements of the synthetic rear face, leaving the candidate
 // closure intact. This specifically models a scan without rear coverage.
 for(size_t i=0;i<good.support.size();i+=3)if(good.normals[i+2]>-0.5){
  noRear.support.insert(noRear.support.end(),good.support.begin()+i,good.support.begin()+i+3);
  noRear.normals.insert(noRear.normals.end(),good.normals.begin()+i,good.normals.begin()+i+3);
 }
 std::vector<uint8_t> mask(noRear.triangles.size()/3);
 auto select=sda_hrtf_select_measured_faces(noRear.view(),mask.data(),mask.size(),nullptr,nullptr);
 require(select.code==0,"No-rear scan must allow measured face selection");
 auto partial=selected(noRear,mask);
 require(partial.triangles.size()==good.triangles.size()*5/6,"Rear fill must be discarded, front/sides retained");
 for(size_t f=0;f<mask.size();++f)if(mask[f]){
  bool rear=true;for(int j=0;j<3;++j)rear=rear&&noRear.vertices[3*noRear.triangles[3*f+j]+2]<-0.009f;
  require(!rear,"Unmeasured rear closure must never appear in review");
 }
 auto measured=sda_hrtf_screen_measured_surface(partial.view(),nullptr,nullptr);
 require(measured.code==0&&measured.enclosed_volume==0&&measured.support_distance<=0.0015,"Partial measured review must succeed without acoustic volume");
 check(partial,1,"Partial measurement must not pass acoustic gate","two-manifold");
 require(sda_hrtf_screen_measured_surface(noRear.view(),nullptr,nullptr).code==1,"Measurement gate must reject untrimmed unsupported rear");
 auto winding=partial;std::swap(winding.triangles[0],winding.triangles[1]);
 require(sda_hrtf_screen_measured_surface(winding.view(),nullptr,nullptr).code==1,"Measurement gate must retain winding checks");
 auto nonmanifold=partial;nonmanifold.triangles.insert(nonmanifold.triangles.end(),partial.triangles.begin(),partial.triangles.begin()+3);
 require(sda_hrtf_screen_measured_surface(nonmanifold.view(),nullptr,nullptr).code==1,"Measurement gate must retain manifold checks");
 auto bad=partial;bad.support[0]=std::numeric_limits<float>::quiet_NaN();
 require(sda_hrtf_screen_measured_surface(bad.view(),nullptr,nullptr).code==1,"Partial review must reject invalid support");
 require(sda_hrtf_select_measured_faces(noRear.view(),mask.data(),mask.size()-1,nullptr,nullptr).code==1,"Selection must bound mask writes");
 require(sda_hrtf_select_measured_faces(noRear.view(),mask.data(),mask.size(),[](void*){return 1;},nullptr).code==2,"Cancel partial selection");
 int calls=0;require(sda_hrtf_select_measured_faces(noRear.view(),mask.data(),mask.size(),[](void *p){return int(++*static_cast<int*>(p)>=3);},&calls).code==2,"Cancel selection during work");
 require(sda_hrtf_screen_measured_surface(partial.view(),[](void*){return 1;},nullptr).code==2,"Cancel partial screen");
 require(sda_hrtf_screen_measured_surface(partial.view(),nullptr,nullptr).code==0,"Recover after cancelled partial screen");
 auto invalid=noRear;invalid.triangles[0]=UINT32_MAX;
 require(sda_hrtf_select_measured_faces(invalid.view(),mask.data(),mask.size(),nullptr,nullptr).code==1,"Selection must reject invalid face indices");
 auto opposing=noRear;for(float &n:opposing.normals)n=-n;
 require(sda_hrtf_select_measured_faces(opposing.view(),mask.data(),mask.size(),nullptr,nullptr).code==1,"Selection must not accept opposite measured normals");
}
int main(){try{
 require(sda_hrtf_scan_point_limit(0)==0,"Unknown scan budget must not allocate");
 require(sda_hrtf_scan_point_limit(100000*uint64_t(512)-1)==0,"Reject too-small scan budget");
 require(sda_hrtf_scan_point_limit(256*uint64_t(1024)*1024)==524288,"Derive points from working memory");
 require(sda_hrtf_scan_point_limit(768*uint64_t(1024)*1024)==SDA_HRTF_MAXIMUM_SUPPORT_POINTS,"Bound large device budget");
 require(sda_hrtf_scan_point_limit(UINT64_MAX)==SDA_HRTF_MAXIMUM_SUPPORT_POINTS,"No overflow on large reported budget");
 auto good=cube();check(good,0,"Closed supported synthetic cube");measurementTests(good);
 auto disconnected=good;uint32_t offset=uint32_t(good.vertices.size()/3);
 for(size_t i=0;i<good.vertices.size();++i)disconnected.vertices.push_back(good.vertices[i]+(i%3==0?0.05f:0.0f));
 for(uint32_t i:good.triangles)disconnected.triangles.push_back(i+offset);
 check(disconnected,1,"Reject disconnected surfaces","disconnected");
 auto nonmanifold=good;nonmanifold.triangles.insert(nonmanifold.triangles.end(),good.triangles.begin(),good.triangles.begin()+3);check(nonmanifold,1,"Reject three-face edge","two-manifold");
 auto open=good;open.triangles.resize(open.triangles.size()-3);check(open,1,"Reject open mesh","two-manifold");
 auto winding=good;std::swap(winding.triangles[0],winding.triangles[1]);check(winding,1,"Reject reversed face","winding");
 auto reversed=good;for(size_t i=0;i<reversed.triangles.size();i+=3)std::swap(reversed.triangles[i],reversed.triangles[i+1]);check(reversed,1,"Reject inward surface","positive");
 auto invalid=good;invalid.triangles[0]=uint32_t(invalid.vertices.size());check(invalid,1,"Reject out-of-range face","index");
 auto nan=good;nan.vertices[0]=std::numeric_limits<float>::quiet_NaN();check(nan,1,"Reject non-finite geometry","coordinates");
 auto fold=good;bool moved=false;for(size_t i=0;i<fold.vertices.size();i+=3)if(std::abs(fold.vertices[i])<0.0001&&std::abs(fold.vertices[i+1])<0.0001&&fold.vertices[i+2]>0.009){fold.vertices[i]+=0.006f;moved=true;break;}require(moved,"No synthetic fold vertex");check(fold,1,"Reject self-intersecting folded surface","intersects");
 auto holes=good;holes.support.clear();holes.normals.clear();
 for(size_t i=0;i<good.support.size();i+=3)if(!(good.support[i+2]>0.009&&std::hypot(good.support[i],good.support[i+1])<0.005))for(int j=0;j<3;++j){holes.support.push_back(good.support[i+j]);holes.normals.push_back(good.normals[i+j]);}
 check(holes,1,"Reject unmeasured Poisson closure","measured depth support");
 auto badNormals=good;for(float &v:badNormals.normals)v=-v;check(badNormals,1,"Reject unsupported orientation","measured depth support");
 auto huge=good.view();huge.support_count=SDA_HRTF_MAXIMUM_SUPPORT_POINTS+size_t(1);require(sda_hrtf_screen_head_mesh(huge,nullptr,nullptr).code==3,"Enforce input bounds before reading");
 require(sda_hrtf_screen_head_mesh(good.view(),[](void*){return 1;},nullptr).code==2,"Cancel before processing");
 int callbacks=0;require(sda_hrtf_screen_head_mesh(good.view(),[](void *p){return int(++*static_cast<int*>(p)>=3);},&callbacks).code==2,"Cancel during verification");
 check(good,0,"Recovery after rejected/cancelled meshes");
 // Actual arrays at the shared maximum: many measurements in the same cells
 // used to exceed the work guard despite having valid nearby support.
 auto dense=good;const auto originalSupport=good.support,originalNormals=good.normals;
 dense.support.resize(3*size_t(SDA_HRTF_MAXIMUM_SUPPORT_POINTS));dense.normals.resize(dense.support.size());
 for(size_t i=0;i<dense.support.size();++i){dense.support[i]=originalSupport[i%originalSupport.size()];dense.normals[i]=originalNormals[i%originalNormals.size()];}
 check(dense,0,"Exact dense support search at maximum scan budget");
 callbacks=0;require(sda_hrtf_screen_head_mesh(dense.view(),[](void *p){return int(++*static_cast<int*>(p)>=1500);},&callbacks).code==2,"Cancel dense support indexing");
 auto largeMesh=cube(256);check(largeMesh,0,"Dense surface above legacy 80k vertex limit");
 std::cout<<"{\"partialMeasuredNoRearPassed\":true,\"acousticClosedGatePreserved\":true,\"denseMeshCapacityPassed\":true,\"denseMeshVertices\":393218,\"denseMeshFaces\":786432,\"scanBudgetTestsPassed\":true,\"denseSupportPoints\":1500000,\"geometricChecksPassed\":true,\"selfIntersectionRejected\":true,\"unmeasuredClosureRejected\":true,\"cancellation\":true,\"physicalScanTested\":false,\"acousticAccuracyValidated\":false}"<<std::endl;
 return 0;
}catch(const std::exception &e){std::cerr<<e.what()<<std::endl;return 1;}}
