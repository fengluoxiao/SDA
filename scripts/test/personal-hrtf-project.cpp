// Synthetic 12 mm cube: format/lifecycle regression, NEVER human anatomy.
#include "SdaHrtfProject.h"
#include "SdaHrtfSolver.h"
#include <algorithm>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <array>
#include <cmath>
#include <iostream>
#include <map>
#include <limits>
#include <memory>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>
namespace fs=std::filesystem;
struct Fixture {
 std::vector<float> vertices,support,normals;std::vector<uint32_t> triangles;
 SdaHrtfMeshView view(){return {vertices.data(),vertices.size()/3,triangles.data(),triangles.size()/3,support.data(),normals.data(),support.size()/3};}
};
void require(bool ok,const char *message){if(!ok)throw std::runtime_error(message);}
Fixture cube(){
 Fixture f;std::map<std::array<int,3>,uint32_t> ids;constexpr int n=2;
 auto vertex=[&](std::array<int,3> p){auto result=ids.emplace(p,uint32_t(ids.size()));if(result.second)for(int v:p)f.vertices.push_back(float(v)*0.006f-0.006f);return result.first->second;};
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
  for(int a=0;a<=3;++a)for(int b=0;b<=3-a;++b)for(int j=0;j<3;++j){
   f.support.push_back(float(v[0][j]+(v[1][j]-v[0][j])*a/3+(v[2][j]-v[0][j])*b/3));f.normals.push_back(float(normal[j]/length));
  }
 }
 return f;
}

static std::string read(const fs::path &p){std::ifstream s(p,std::ios::binary);return {std::istreambuf_iterator<char>(s),{}};}
static void samplingPlans(const fs::path &root,const SdaHrtfProjectInput &fixture){
 constexpr uint64_t mib=1024*1024, gib=1024*mib;
 struct Case {int ram,roomMiB,az,el,bins;};
 const Case cases[]={{4,1536,16,8,256},{6,1536,24,12,512},{12,3072,32,16,1024},
                     {16,4096,40,16,1024},{24,6144,40,18,1024},{32,8192,40,18,1024}};
 auto plan=[](uint64_t room,uint64_t ram,size_t nodes=8718,size_t faces=17432){return sda_hrtf_sampling_plan(room,ram,nodes,faces);};
 auto dirs=[](const SdaHrtfSamplingPlan &p){return p.azimuth_steps*(p.elevation_steps-1)+2;};
 for(const auto &c:cases){
  auto p=plan(c.roomMiB*mib,c.ram*gib);
  require(p.tier>0 && p.azimuth_steps==c.az && p.elevation_steps==c.el && p.bins==c.bins,"Unexpected resource-selected grid");
  require(p.numerical_budget_bytes==c.ram*gib/8,"Incorrect physical/headroom-limited solve budget");
  auto input=fixture;input.azimuth_steps=p.azimuth_steps;input.elevation_steps=p.elevation_steps;input.bins=p.bins;
  auto dir=root/("capacity-"+std::to_string(c.ram));fs::create_directory(dir);
  auto result=sda_hrtf_prepare_project(dir.string().c_str(),input,nullptr,nullptr);
  require(result.code==0 && result.directions==size_t(dirs(p)),"Selected grid not accepted by project writer");
  auto project=dir/"AcousticProject";
  std::ifstream grid(project/"EvaluationGrids/FullSphere/Nodes.txt"),faces(project/"EvaluationGrids/FullSphere/Elements.txt");
  size_t n=0,f=0;require((grid>>n) && n==result.directions && (faces>>f) && f==2*(n-2),"Grid file dimensions differ from plan");
  std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)> solver(sda_hrtf_create_for_resources(project.string().c_str(),c.roomMiB*mib,c.ram*gib),sda_hrtf_solver_destroy);
  require(bool(solver),"Cannot open adaptive project");
  for(int ear:{1,2}){
   auto relative="NumCalc/source_"+std::to_string(ear);auto earDir=project/relative;
   auto previousLog=earDir/("NC"+std::to_string(p.bins)+"-"+std::to_string(p.bins)+".out");
   std::ofstream(previousLog)<<"previous log";std::ofstream(earDir/"Memory.txt")<<"previous estimate";
   for(int k:{p.bins,1,p.bins+1}){
    auto check=sda_hrtf_check_frequency_capacity(solver.get(),relative.c_str(),k);
    require(check.code==(k>p.bins?1:0) && !check.remaining_array_bytes,"Invalid frequency preflight or resource leak");
   }
   require(!fs::exists(earDir/"be.out") && !fs::exists(earDir/"NC1-1.out") && read(previousLog)=="previous log" && read(earDir/"Memory.txt")=="previous estimate","Preflight wrote or overwrote output");
   std::ifstream nc(project/("NumCalc/source_"+std::to_string(ear)+"/NC.inp"));std::string line;
   for(int k=0;k<3;++k)std::getline(nc,line);
   int method=0,count=0,id=-1,points=0;nc>>method>>count;std::getline(nc,line);
   require(method==1 && count==p.bins && (nc>>id>>points) && id==0 && points==p.bins+1,"Ear project not using selected frequency count");
   for(int k=0;k<=p.bins;++k){double t,hz,im;require((nc>>t>>hz>>im) && std::abs(hz-24000.*k/p.bins)<1e-8 && im==0,"Ear frequency grid mismatch");}
   // Exercise the low, 16 GiB and format-limited grids, not just their manifests.
   if(c.ram==4 || c.ram==16 || c.ram==24){
    auto source="NumCalc/source_"+std::to_string(ear);const int step=p.bins/4; // 6 kHz on all grids
    auto r=sda_hrtf_solver_run(solver.get(),source.c_str(),c.ram==4?1:step,c.ram==4?p.bins:step,0,nullptr,nullptr);
    if(r.code)throw std::runtime_error(r.message);
    require(!r.remaining_array_bytes,"Adaptive frequency retained arrays");
    std::ifstream eval(project/source/"be.out"/("be."+std::to_string(step))/"pEvalGrid");
    std::getline(eval,line);int groups=0,g=0;size_t rows=0;
    require((eval>>groups>>g>>rows) && groups==1 && g==1 && rows==n,"Wrong adaptive pressure shape");
    std::set<int> seen;
    for(size_t i=0;i<n;++i){double re,im;int node;require((eval>>node>>re>>im) && node>=400000 && size_t(node-400000)<n && seen.insert(node).second && std::isfinite(re) && std::isfinite(im) && (re!=0 || im!=0),"Missing or invalid adaptive pressure");}
   }
  }
  if(c.ram==4){
   const size_t stride=4*size_t(p.bins);
   std::vector<int32_t> ids(n);for(size_t i=0;i<n;++i)ids[i]=400000+int32_t(i);
   std::vector<double> bank(n*stride);std::vector<float> hrir(stride);
   auto r=sda_hrtf_read_pressure(solver.get(),ids.data(),n,p.bins,bank.data(),bank.size());
   if(r.code)throw std::runtime_error(r.message);
   for(size_t i=0;i<n;++i){
    r=sda_hrtf_make_hrir(solver.get(),bank.data()+i*stride,stride,p.bins,result.source_areas,1.2,p.bins/2,hrir.data(),hrir.size());
    if(r.code)throw std::runtime_error(r.message);
    for(size_t ear:{0,1}){double energy=0;for(size_t j=0;j<stride/2;++j){double x=hrir[ear*stride/2+j];energy+=x*x;}require(std::isfinite(energy) && energy>1e-16,"Invalid full baseline HRIR");}
   }
  }
 }
 const uint64_t max=std::numeric_limits<uint64_t>::max();
 auto wide=plan(max,max),base=plan(1536*mib,4*gib);
 require(dirs(wide)==682 && wide.bins==1024 && wide.numerical_budget_bytes==4*gib,"Future capacity must saturate without overflow");
 require(plan(0,32*gib).tier==0 && plan(8*gib,0).tier==0 && plan(512*mib-1,32*gib).tier==0,"Never guess missing/insufficient headroom");
 require(plan(768*mib,32*gib).tier==base.tier && plan(1536*mib,32*gib).tier==2 && plan(3*gib,32*gib).tier==3,"Multitasking headroom must override physical capacity");
 require(plan(16*gib,4*gib).tier==base.tier,"Available RAM cannot bypass the physical capacity gate");
 require(plan(4*gib,15*gib).tier==4 && plan(6*gib,22*gib).tier==5,"Intermediate reported capacities must not require exact RAM buckets");
 require(plan(16*gib,32*gib,20000-114,20000-224).tier==1 && !plan(16*gib,32*gib,20000-113,4).tier && !plan(16*gib,32*gib,4,20000-223).tier,"Off-by-one in remaining solver indices");
 require(plan(max,max,8718,20000-1200).tier==4 && plan(max,max,8718,20000-1199).tier==3,"Dense mesh must leave room for grid elements");
 require(!plan(max,max,3,4).tier && !plan(max,max,4,3).tier && !plan(max,max,std::numeric_limits<size_t>::max(),4).tier && !plan(max,max,4,std::numeric_limits<size_t>::max()).tier,"Invalid mesh sizes must not wrap");
 // Sweep nonstandard capacities and arbitrary headroom independently; neither
 // increasing resource may reduce sampling density or skip the archive bound.
 int previous[97]{};
 for(int phys=3;phys<=64;++phys){int last=0;for(int j=0;j<=96;++j){
  auto p=plan(j*128*mib,phys*gib);
  require(p.tier>=last && p.tier>=previous[j] && p.tier<=wide.tier,"Non-monotonic capacity policy");
  last=previous[j]=p.tier;
 }}
}
static void nodesMatch(const fs::path &p,const Fixture &f,const SdaHrtfProjectResult &result){
 std::istringstream s(read(p));size_t n;require(bool(s>>n)&&n==f.vertices.size()/3,"Node count changed");
 for(size_t i=0;i<n;++i){size_t id;double x,y,z;require(bool(s>>id>>x>>y>>z)&&id==i,"Node order changed");require(std::abs(x-(f.vertices[3*i]-result.origin[0]))<1e-12&&std::abs(y-(f.vertices[3*i+1]-result.origin[1]))<1e-12&&std::abs(z-(f.vertices[3*i+2]-result.origin[2]))<1e-12,"Measured geometry was scaled or changed");}
}
int main(int argc,char **argv)try{
 require(argc==2,"Pass a fresh temporary test directory");fs::path root=fs::absolute(argv[1]);require(!fs::exists(root),"Test directory must be fresh");fs::create_directories(root);
 {std::ofstream sentinel(root/"unrelated");sentinel<<"keep";}
 auto f=cube();uint32_t left[]={24,25},right[]={16};
 SdaHrtfProjectInput in{};in.mesh=f.view();in.left_faces=left;in.left_face_count=2;in.right_faces=right;in.right_face_count=1;in.front[0]=1;in.up[2]=1;in.bins=16;in.azimuth_steps=16;in.elevation_steps=4;in.reference_radius=1.2;
 samplingPlans(root,in);
 auto call=[&](SdaHrtfProjectInput v,SdaHrtfMeshCancelled c=nullptr,void *ctx=nullptr){return sda_hrtf_prepare_project(root.string().c_str(),v,c,ctx);};
 auto reject=[&](SdaHrtfProjectInput v){auto result=call(v);require(result.code!=0,"Invalid preparation accepted");require(!fs::exists(root/"AcousticProject")&&!fs::exists(root/".preparing-acoustic"),"Rejected preparation left partial files");require(read(root/"unrelated")=="keep","Preparation touched unrelated files");};
 auto bad=in;bad.bins=17;reject(bad);bad=in;bad.elevation_steps=5;reject(bad);bad=in;bad.bins=2048;bad.azimuth_steps=64;bad.elevation_steps=32;reject(bad);
 bad=in;bad.mesh.vertex_count=20001;reject(bad);bad=in;bad.front[0]=2;reject(bad);bad=in;bad.up[0]=1;reject(bad);bad=in;bad.front[0]=std::numeric_limits<double>::quiet_NaN();reject(bad);
 uint32_t duplicates[]={24,24};bad=in;bad.left_faces=duplicates;reject(bad);
 uint32_t disconnected[]={24,31};bad=in;bad.left_faces=disconnected;reject(bad);
 bad=in;bad.right_faces=left;bad.right_face_count=2;reject(bad);
 bad=in;bad.left_faces=right;bad.left_face_count=1;bad.right_faces=left;bad.right_face_count=2;reject(bad);
 bad=in;bad.left_face_count=0;reject(bad);
 require(call(in,[](void*){return 1;}).code==2,"Early cancellation failed");
 auto stopOnStage=[](void *p){return fs::exists(*static_cast<fs::path*>(p)/".preparing-acoustic/ObjectMeshes/Reference/Nodes.txt")?1:0;};
 require(call(in,stopOnStage,&root).code==2,"Write-stage cancellation failed");require(!fs::exists(root/"AcousticProject")&&!fs::exists(root/".preparing-acoustic"),"Cancellation retained private temporary data");
 fs::create_directory(root/".preparing-acoustic");{std::ofstream s(root/".preparing-acoustic"/"other-owner");s<<"keep";}
 require(call(in).code!=0 && read(root/".preparing-acoustic"/"other-owner")=="keep","Another invocation's stage was removed");fs::remove(root/".preparing-acoustic"/"other-owner");fs::remove(root/".preparing-acoustic");
 // Failure after acquiring staging must clean only our stage, including
 // failures occurring after some files have already been written.
 auto breakWrite=[](void *p){auto stage=*static_cast<fs::path*>(p)/".preparing-acoustic";if(fs::exists(stage)&&!fs::exists(stage/"ObjectMeshes")){std::ofstream s(stage/"ObjectMeshes");s<<"force I/O failure";}return 0;};
 require(call(in,breakWrite,&root).code!=0 && !fs::exists(root/".preparing-acoustic") && !fs::exists(root/"AcousticProject"),"I/O failure retained partial preparation");
 std::error_code symlinkError;fs::create_directory_symlink(root/"not-created",root/"AcousticProject",symlinkError);
 if(!symlinkError){require(call(in).code!=0 && fs::is_symlink(fs::symlink_status(root/"AcousticProject")),"Dangling existing target was replaced");fs::remove(root/"AcousticProject");}
 auto result=call(in);if(result.code)throw std::runtime_error(result.message);
 require(result.directions==50,"Full sphere direction count changed");
 require(std::abs(result.source_areas[0]-2*result.source_areas[1])<1e-12,"Ear source areas were equalized");
 fs::path project=root/"AcousticProject";nodesMatch(project/"ObjectMeshes/Reference/Nodes.txt",f,result);
 const auto before=read(project/"NumCalc/source_1/NC.inp");require(call(in).code!=0&&before==read(project/"NumCalc/source_1/NC.inp"),"Existing project was overwritten");
 require(read(project/"sda-project.json").find("\"acousticAccuracyValidated\":false")!=std::string::npos,"Synthetic preparation claimed acoustic qualification");
 std::istringstream grid(read(project/"EvaluationGrids/FullSphere/Nodes.txt"));size_t count;require(bool(grid>>count)&&count==50,"Wrong grid node count");std::map<int,std::array<double,3>> points;std::vector<int32_t> ids;
 for(size_t i=0;i<count;++i){int id;std::array<double,3> p;require(bool(grid>>id>>p[0]>>p[1]>>p[2])&&id==400000+int(i),"Unstable evaluation IDs");require(std::abs(std::hypot(std::hypot(p[0],p[1]),p[2])-1.2)<1e-12,"Nonuniform reference radius");points[id]=p;ids.push_back(id);}
 std::istringstream elements(read(project/"EvaluationGrids/FullSphere/Elements.txt"));size_t triangles;require(bool(elements>>triangles)&&triangles==96,"Wrong grid face count");std::map<std::pair<int,int>,int> edges;
 for(size_t i=0;i<triangles;++i){int id,a,b,c,type,x,y;require(bool(elements>>id>>a>>b>>c>>type>>x>>y)&&type==2,"Wrong grid face type");auto p=points.at(a),q=points.at(b),r=points.at(c);double normal[3];for(int j=0;j<3;++j)normal[j]=(q[(j+1)%3]-p[(j+1)%3])*(r[(j+2)%3]-p[(j+2)%3])-(q[(j+2)%3]-p[(j+2)%3])*(r[(j+1)%3]-p[(j+1)%3]);require(normal[0]*p[0]+normal[1]*p[1]+normal[2]*p[2]>0,"Inward evaluation triangle");for(auto e:{std::pair<int,int>{a,b},{b,c},{c,a}})++edges[std::minmax(e.first,e.second)];}
 for(auto e:edges)require(e.second==2,"Open evaluation sphere");
 // Prove a rigid rotation/translation of measured coordinates yields the same
 // head-frame nodes. Normals transform as vectors, not points. No rescaling.
 auto rotated=f;auto transform=[](std::vector<float>&v,bool position){for(size_t i=0;i<v.size();i+=3){float x=v[i],y=v[i+1];v[i]=-y+(position?.125f:0);v[i+1]=x+(position?.25f:0);v[i+2]+=position?.5f:0;}};
 transform(rotated.vertices,true);transform(rotated.support,true);transform(rotated.normals,false);bad=in;bad.mesh=rotated.view();bad.front[0]=0;bad.front[1]=1;
 fs::create_directory(root/"rotated");auto rr=sda_hrtf_prepare_project((root/"rotated").string().c_str(),bad,nullptr,nullptr);require(rr.code==0,"Rigid coordinate conversion rejected");
 std::istringstream original(read(project/"ObjectMeshes/Reference/Nodes.txt")),converted(read(root/"rotated/AcousticProject/ObjectMeshes/Reference/Nodes.txt"));size_t n1,n2;original>>n1;converted>>n2;require(n1==n2,"Rotation changed node count");for(size_t i=0;i<n1;++i){int a,b;double x[3],y[3];original>>a>>x[0]>>x[1]>>x[2];converted>>b>>y[0]>>y[1]>>y[2];for(int j=0;j<3;++j)require(std::abs(x[j]-y[j])<5e-8,"Rigid transform corrupted head coordinates");}
 // Consume the actual generated project with the real embedded solver/reader.
 std::unique_ptr<SdaHrtfSolver,decltype(&sda_hrtf_solver_destroy)> solver(sda_hrtf_solver_create(project.string().c_str(),32*1024*1024),sda_hrtf_solver_destroy);require(bool(solver),"Cannot open prepared project");
 for(int ear=1;ear<=2;++ear){auto r=sda_hrtf_solver_run(solver.get(),("NumCalc/source_"+std::to_string(ear)).c_str(),1,16,0,nullptr,nullptr);if(r.code)throw std::runtime_error(r.message);require(r.remaining_array_bytes==0,"Solver leaked numerical arrays");}
 std::vector<double> bank(50*4*16);auto readResult=sda_hrtf_read_pressure(solver.get(),ids.data(),50,16,bank.data(),bank.size());if(readResult.code)throw std::runtime_error(readResult.message);
 std::vector<float> impulse(64);for(int d=0;d<50;++d){auto r=sda_hrtf_make_hrir(solver.get(),bank.data()+d*64,64,16,result.source_areas,1.2,8,impulse.data(),impulse.size());if(r.code)throw std::runtime_error(r.message);}
 require(read(root/"unrelated")=="keep","Solve touched unrelated files");
 std::cout<<"{\"passed\":true,\"preparedCapacityGiB\":[4,6,12,16,24,32],\"bilateral6kHzDirections\":[114,602,682],\"completeBaselineHRIRs\":114,\"baselineBinsPerEar\":256,\"resourceSweepCases\":6014,\"actualGeneratedProjectSolved\":true,\"directions\":50,\"independentSourceAreas\":true,\"rigidFrameChecked\":true,\"fullSphereTopologyChecked\":true,\"cancellationCleanup\":true,\"overwriteRejected\":true,\"physicalScanTested\":false,\"acousticAccuracyValidated\":false}"<<std::endl;return 0;
}catch(const std::exception &e){std::cerr<<e.what()<<std::endl;return 1;}
