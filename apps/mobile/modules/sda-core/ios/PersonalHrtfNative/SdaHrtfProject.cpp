#include "SdaHrtfProject.h"
#include "SdaHrtfSolver.h"
#include "SdaHrtfMeshInternal.hpp"
#include "MeshOpt/meshoptimizer.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <locale>
#include <map>
#include <set>
#include <stdexcept>
#include <vector>
namespace {
namespace fs=std::filesystem;
constexpr size_t nativeDimensionLimit=20000;
constexpr uint64_t MiB=1024ull*1024;
constexpr uint64_t maxResponseBytes=16*MiB;
// Bounds for the generated (not imported) archive. Foundation emits unwrapped,
// unescaped base64 per direction; include both JSON records, SHA and header.
// zlib default window/memLevel deflateBound plus the gzip wrapper/trailer.
bool automaticGridFits(size_t vertices,size_t faces,int az,int el,int bins,uint64_t budget,uint64_t headroom) {
 const size_t rings=size_t(az)*(el-1), dirs=rings+2;
 if(dirs>nativeDimensionLimit-vertices || 2*rings>nativeDimensionLimit-faces)return false;
 const uint64_t perDirection=4ull*bins*sizeof(float), samples=dirs*perDirection;
 const uint64_t pressure=dirs*4ull*bins*sizeof(double);
 const uint64_t json=16*1024+dirs*(4*((perDirection+2)/3)+512);
 const uint64_t gzip=json+(json>>12)+(json>>14)+(json>>25)+32;
 return samples<=maxResponseBytes && json<=24*MiB && gzip<=16*MiB &&
        pressure<=64*MiB && pressure+4*MiB<=budget && pressure+6*samples+128*MiB<=headroom;
}
constexpr double pi=3.14159265358979323846;
struct Failure:std::runtime_error { int code; Failure(int c,const char *s):std::runtime_error(s),code(c){} };
using V=std::array<double,3>;
V add(V a,V b){for(int i=0;i<3;++i)a[i]+=b[i];return a;}
V sub(V a,V b){for(int i=0;i<3;++i)a[i]-=b[i];return a;}
V mul(V a,double b){for(double &v:a)v*=b;return a;}
double dot(V a,V b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
V cross(V a,V b){return {a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]};}
double norm(V a){return std::sqrt(dot(a,a));}
void require(bool ok,const char *text){if(!ok)throw Failure(1,text);}
struct Build {
 SdaHrtfProjectInput in; SdaHrtfMeshCancelled cancelled; void *context;
 SdaHrtfProjectResult reply{};
 V front{},up{},left{},origin{};
 struct Patch {double area=0;V center{},normal{};std::vector<uint32_t> faces;};
 void check() {
  if(cancelled&&cancelled(context))throw Failure(2,"Local acoustic project preparation cancelled");
 }
 V vertex(uint32_t index){const float *p=in.mesh.vertices+3*index;return {p[0],p[1],p[2]};}
 Patch patch(const uint32_t *ids,size_t count) {
  require(ids && count && count<=1024 && count<=in.mesh.triangle_count,"Missing or oversized confirmed ear patch");
  Patch p;std::set<uint32_t> unique;
  for(size_t i=0;i<count;++i) {
   check();require(ids[i]<in.mesh.triangle_count && unique.insert(ids[i]).second,"Invalid or duplicate ear patch triangle");
   p.faces.push_back(ids[i]);
   const uint32_t *t=in.mesh.triangles+3*ids[i];V a=vertex(t[0]),b=vertex(t[1]),c=vertex(t[2]);
   V n=cross(sub(b,a),sub(c,a));double area=norm(n)/2;
   p.area+=area;p.center=add(p.center,mul(add(add(a,b),c),area/3));p.normal=add(p.normal,mul(n,.5));
  }
  require(std::isfinite(p.area) && p.area>=1e-9 && p.area<=.001,"Ear patch area exceeds response conversion bounds");
  p.center=mul(p.center,1/p.area);p.normal=mul(p.normal,1/p.area);
  // Each selected source must be one disk of edge-connected triangles, not
  // distant islands or a ring enclosing unselected geometry. Never fill gaps.
  std::map<std::pair<uint32_t,uint32_t>,std::vector<size_t>> edges;
  std::set<uint32_t> vertices;
  for(size_t i=0;i<count;++i) {
   const uint32_t *t=in.mesh.triangles+3*p.faces[i];
   for(int j=0;j<3;++j){vertices.insert(t[j]);auto edge=std::minmax(t[j],t[(j+1)%3]);edges[edge].push_back(i);}
  }
  std::vector<std::vector<size_t>> neighbors(count);
  std::map<uint32_t,std::vector<uint32_t>> boundary;
  for(const auto &entry:edges) {
   const auto &adj=entry.second;
   if(adj.size()==2){neighbors[adj[0]].push_back(adj[1]);neighbors[adj[1]].push_back(adj[0]);}
   else {require(adj.size()==1,"Nonmanifold ear source patch");boundary[entry.first.first].push_back(entry.first.second);boundary[entry.first.second].push_back(entry.first.first);}
  }
  std::vector<bool> visited(count);std::vector<size_t> queue{0};visited[0]=true;
  for(size_t i=0;i<queue.size();++i)for(auto next:neighbors[queue[i]])if(!visited[next]){visited[next]=true;queue.push_back(next);}
  require(queue.size()==count && !boundary.empty(),"Ear source patch must be edge-connected and open");
  for(const auto &entry:boundary)require(entry.second.size()==2,"Ear source patch boundary branches");
  std::set<uint32_t> reached;std::vector<uint32_t> loop{boundary.begin()->first};reached.insert(loop[0]);
  for(size_t i=0;i<loop.size();++i)for(auto next:boundary.at(loop[i]))if(reached.insert(next).second)loop.push_back(next);
  require(reached.size()==boundary.size() && vertices.size()+count==edges.size()+1,"Ear source patch must be one disk without holes");
  return p;
 }
 SdaHrtfProjectResult run(const char *root,const SdaHrtfMeshView *review=nullptr,size_t measuredFaces=0,double *deviation=nullptr) {
  check();require(root && *root,"Missing private acoustic job directory");
  if(in.mesh.vertex_count>nativeDimensionLimit || in.mesh.triangle_count>nativeDimensionLimit)throw Failure(3,"Measured mesh exceeds embedded solver dimensions; no automatic detail removal");
  require(in.bins>=16 && in.bins<=2048 && (in.bins&(in.bins-1))==0,"Invalid uniform FFT frequency count");
  require(in.azimuth_steps>=8 && in.azimuth_steps<=64 && in.azimuth_steps%8==0 && in.elevation_steps>=4 && in.elevation_steps<=32 && in.elevation_steps%2==0,"Invalid full sphere evaluation grid");
  const size_t directions=size_t(in.azimuth_steps)*(in.elevation_steps-1)+2;
  if(directions<32 || directions>2048 || directions*4*in.bins*sizeof(float)>maxResponseBytes)throw Failure(3,"Evaluation grid exceeds response archive bounds");
  const size_t allGridTriangles=2*size_t(in.azimuth_steps)*(in.elevation_steps-1);
  if(in.mesh.vertex_count+directions>nativeDimensionLimit || in.mesh.triangle_count+allGridTriangles>nativeDimensionLimit)throw Failure(3,"Mesh plus evaluation grid exceeds native solver dimensions");
  require(std::isfinite(in.reference_radius) && in.reference_radius>=.25 && in.reference_radius<=10,"Invalid common evaluation radius");
  for(int i=0;i<3;++i){require(std::isfinite(in.front[i]) && std::isfinite(in.up[i]),"Nonfinite head coordinate frame");front[i]=in.front[i];up[i]=in.up[i];}
  require(std::abs(norm(front)-1)<1e-6 && std::abs(norm(up)-1)<1e-6 && std::abs(dot(front,up))<1e-6,"Head axes must be measured, unit and orthogonal");
  left=cross(up,front);
  if(!review){auto screened=sda_hrtf_screen_head_mesh(in.mesh,cancelled,context);if(screened.code)throw Failure(screened.code,screened.message);}
  Patch l=patch(in.left_faces,in.left_face_count),r=patch(in.right_faces,in.right_face_count);
  if(review){double ears[]={l.center[0],l.center[1],l.center[2],r.center[0],r.center[1],r.center[2]};
   auto screened=sda_screen_prepared_surface(in.mesh,measuredFaces,*review,ears,*deviation,cancelled,context);if(screened.code)throw Failure(screened.code,screened.message);}
  std::set<uint32_t> lfaces(l.faces.begin(),l.faces.end());
  for(auto face:r.faces)require(!lfaces.count(face),"Left and right ear source patches overlap");
  V separation=sub(l.center,r.center);double distance=norm(separation);
  require(distance>=.01 && distance<=.30 && dot(separation,left)>.8*distance,"Confirmed ears disagree with left/right head frame");
  require(dot(l.normal,left)>.2 && dot(r.normal,left)<-.2,"Ear patches must face opposite measured lateral directions");
  origin=mul(add(l.center,r.center),.5);
  for(size_t i=0;i<in.mesh.vertex_count;++i){if(i%1024==0)check();require(norm(sub(vertex(uint32_t(i)),origin))<in.reference_radius*.8,"Evaluation sphere must enclose measured surface with clearance");}
  fs::path owner=fs::canonical(root);require(fs::is_directory(owner),"Job root is not a directory");
  const auto target=owner/"AcousticProject",stage=owner/".preparing-acoustic";
  // create_directory is exclusive, including for dangling symlinks. Do not
  // clean up another invocation's staging directory after a failed acquire.
  auto exists=[](const fs::path &p){return fs::symlink_status(p).type()!=fs::file_type::not_found;};
  require(!exists(target),"Acoustic project already exists");
  require(fs::create_directory(stage),"Acoustic project preparation already owns staging directory");
  bool committed=false;
  try {
   require(!exists(target),"Acoustic project already exists");
   auto file=[&](const char *relative,auto write) {
    check();auto path=stage/relative;fs::create_directories(path.parent_path());
    std::ofstream out(path,std::ios::binary);out.imbue(std::locale::classic());out<<std::setprecision(17);
    require(bool(out),"Cannot open private acoustic input");write(out);out.flush();require(bool(out),"Cannot write complete acoustic input");out.close();require(!out.fail(),"Cannot close acoustic input");check();
   };
   file("ObjectMeshes/Reference/Nodes.txt",[&](auto &s){s<<in.mesh.vertex_count<<'\n';for(size_t i=0;i<in.mesh.vertex_count;++i){if(i%1024==0)check();V p=sub(vertex(uint32_t(i)),origin);s<<i<<' '<<dot(p,front)<<' '<<dot(p,left)<<' '<<dot(p,up)<<'\n';}});
   file("ObjectMeshes/Reference/Elements.txt",[&](auto &s){s<<in.mesh.triangle_count<<'\n';for(size_t i=0;i<in.mesh.triangle_count;++i){if(i%1024==0)check();const auto *t=in.mesh.triangles+3*i;s<<i<<' '<<t[0]<<' '<<t[1]<<' '<<t[2]<<" 0 0 0\n";}});
   const int az=in.azimuth_steps,rings=in.elevation_steps-1;
   file("EvaluationGrids/FullSphere/Nodes.txt",[&](auto &s){s<<directions<<'\n';int id=400000;s<<id++<<" 0 0 "<<-in.reference_radius<<'\n';for(int j=1;j<=rings;++j){check();double e=-pi/2+pi*j/in.elevation_steps;for(int i=0;i<az;++i){double a=2*pi*i/az;s<<id++<<' '<<in.reference_radius*std::cos(e)*std::cos(a)<<' '<<in.reference_radius*std::cos(e)*std::sin(a)<<' '<<in.reference_radius*std::sin(e)<<'\n';}}s<<id<<" 0 0 "<<in.reference_radius<<'\n';});
   const size_t gridTriangles=2*size_t(az)*rings;
   file("EvaluationGrids/FullSphere/Elements.txt",[&](auto &s){s<<gridTriangles<<'\n';int id=400000;auto triangle=[&](int a,int b,int c){s<<id++<<' '<<a<<' '<<b<<' '<<c<<" 2 0 1\n";};for(int i=0;i<az;++i)triangle(400001+(i+1)%az,400001+i,400000);for(int j=0;j<rings-1;++j){check();for(int i=0;i<az;++i){int a=400001+j*az+i,b=400001+j*az+(i+1)%az;triangle(a,b,b+az);triangle(a,b+az,a+az);}}for(int i=0;i<az;++i)triangle(400001+(rings-1)*az+i,400001+(rings-1)*az+(i+1)%az,400000+int(directions)-1);});
   for(int ear=0;ear<2;++ear) {
    const auto &patchFaces=ear==0?l.faces:r.faces;
    file(ear==0?"NumCalc/source_1/NC.inp":"NumCalc/source_2/NC.inp",[&](auto &s){
     s<<"Mesh2HRTF 1.0.0\nMeasured surface preparation; acoustic accuracy requires validation\n0 0 0 0 7 0\n1 "<<in.bins<<" .000001 0 1 0 0\n0 "<<in.bins+1<<'\n';
     for(size_t k=0;k<=in.bins;++k){if(k%128==0)check();s<<double(k)*1e-6<<' '<<double(k)*24000/in.bins<<" 0\n";}
     s<<"2 "<<in.mesh.triangle_count+gridTriangles<<' '<<in.mesh.vertex_count+directions<<" 0 0 2 1 4 0\n0 0 0 0 0 0 0\n0 0 0 0\n343 1.1839 1 0 0 0 0\nNODES\n../../ObjectMeshes/Reference/Nodes.txt\n../../EvaluationGrids/FullSphere/Nodes.txt\nELEMENTS\n../../ObjectMeshes/Reference/Elements.txt\n../../EvaluationGrids/FullSphere/Elements.txt\nBOUNDARY\n";
     for(auto face:patchFaces)s<<"ELEM "<<face<<" TO "<<face<<" VELO .1 -1 0 -1\n";
     s<<"RETU\nPOST PROCESS\nEND\n";
    });
   }
   file("sda-project.json",[&](auto &s){s<<"{\"schema\":1,\"acousticAccuracyValidated\":false,\"sampleRate\":48000,\"bins\":"<<in.bins<<",\"azimuthSteps\":"<<az<<",\"elevationSteps\":"<<in.elevation_steps<<",\"directions\":"<<directions<<",\"referenceRadius\":"<<in.reference_radius<<",\"sourceAreas\":["<<l.area<<','<<r.area<<"],\"coordinateFrame\":\"x-front-y-left-z-up\"";
    if(review)s<<",\"measuredFaces\":"<<measuredFaces<<",\"boundaryFaces\":"<<in.mesh.triangle_count-measuredFaces<<",\"boundaryModel\":\""<<(measuredFaces==in.mesh.triangle_count?"none":"posterior-computational-baffle")<<"\",\"maxReviewDeviationM\":"<<*deviation;
    s<<"}\n";});
   check();require(!exists(target),"Acoustic project already exists");fs::rename(stage,target);committed=true;
  } catch(...) {if(!committed){std::error_code error;fs::remove_all(stage,error);}throw;}
  reply.directions=directions;reply.source_areas[0]=l.area;reply.source_areas[1]=r.area;
  for(int i=0;i<3;++i)reply.origin[i]=origin[i];
  std::snprintf(reply.message,sizeof reply.message,"Acoustic input prepared; ear measurement accuracy and frequency convergence remain unqualified");return reply;
 }
};
#include "SdaHrtfReviewPreparation.hpp"
}
extern "C" SdaHrtfReviewProjectResult sda_hrtf_prepare_review_project(const char *root,SdaHrtfProjectInput review,uint64_t room,uint64_t physical,SdaHrtfMeshCancelled cancelled,void *context){
 SdaHrtfReviewProjectResult result{};
 try{std::set<ReviewPreparation::Cell> keep;for(int pass=0;;++pass){ReviewPreparation work{review,cancelled,context};if(work.prepare(root,room,physical,result,keep,pass))break;}}
 catch(const Failure &e){result.project.code=e.code;std::snprintf(result.project.message,sizeof result.project.message,"%s",e.what());}
 catch(...){result.project.code=3;std::snprintf(result.project.message,sizeof result.project.message,"Cannot prepare bounded review geometry");}
 return result;
}
extern "C" SdaHrtfSamplingPlan sda_hrtf_sampling_plan(uint64_t headroom,uint64_t physical,size_t vertices,size_t triangles){
 const auto budget=sda_hrtf_solve_budget(headroom,physical);
 if(!budget || vertices<4 || triangles<4 || vertices>nativeDimensionLimit || triangles>nativeDimensionLimit)return {};
 SdaHrtfSamplingPlan best{};
 int az=16,el=8,bins=256;
 uint64_t nextPhysical=5*1024, nextRoom=1280; // MiB, not marketing RAM or a model identifier
 for(int rank=1;az<=64 && el<=32;++rank){
  if(!automaticGridFits(vertices,triangles,az,el,bins,budget,headroom))break;
  best={rank,az,el,bins,budget};
  if(physical/MiB<nextPhysical || headroom/MiB<nextRoom)break;
  // An admission heuristic for serial work, not a request to allocate RAM.
  // Each refinement requires 60% more capacity on BOTH axes. At 1024 bins
  // spend extra capacity on angular detail, not twice as many BEM solves.
  nextPhysical=(nextPhysical*8+4)/5;
  nextRoom=(nextRoom*8+4)/5;
  if(bins<1024){az+=8;el+=4;bins*=2;}
  else if(az==2*el)az+=8;
  else el+=2;
 }
 return best;
}
extern "C" SdaHrtfProjectResult sda_hrtf_prepare_project(const char *root,SdaHrtfProjectInput input,SdaHrtfMeshCancelled cancelled,void *context){
 Build build{input,cancelled,context};
 try{return build.run(root);}
 catch(const Failure &e){build.reply.code=e.code;std::snprintf(build.reply.message,sizeof build.reply.message,"%s",e.what());}
 catch(const std::bad_alloc &){build.reply.code=3;std::snprintf(build.reply.message,sizeof build.reply.message,"Insufficient memory preparing acoustic inputs");}
 catch(...){build.reply.code=1;std::snprintf(build.reply.message,sizeof build.reply.message,"Cannot prepare private local acoustic project");}
 return build.reply;
}
