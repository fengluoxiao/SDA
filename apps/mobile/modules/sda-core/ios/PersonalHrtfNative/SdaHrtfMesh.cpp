#include "SdaHrtfMesh.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <limits>
#include <numeric>
#include <stdexcept>
#include <unordered_map>
#include <vector>
namespace {
constexpr double tolerance=0.0015, maxEdge=0.012, epsilon=1e-10;
struct Error:std::runtime_error {int code;Error(int c,const char *s):std::runtime_error(s),code(c){}};
struct V {double x,y,z;double operator[](int i)const{return i==0?x:i==1?y:z;}};
V operator+(V a,V b){return {a.x+b.x,a.y+b.y,a.z+b.z};}
V operator-(V a,V b){return {a.x-b.x,a.y-b.y,a.z-b.z};}
V operator*(V a,double b){return {a.x*b,a.y*b,a.z*b};}
double dot(V a,V b){return a.x*b.x+a.y*b.y+a.z*b.z;}
V cross(V a,V b){return {a.y*b.z-a.z*b.y,a.z*b.x-a.x*b.z,a.x*b.y-a.y*b.x};}
double norm(V a){return std::sqrt(dot(a,a));}
V load(const float *p){return {p[0],p[1],p[2]};}
bool finite(V p){return std::isfinite(p.x)&&std::isfinite(p.y)&&std::isfinite(p.z)&&std::max({std::abs(p.x),std::abs(p.y),std::abs(p.z)})<=10;}
struct Box {
 V low{1e30,1e30,1e30},high{-1e30,-1e30,-1e30};
 void add(V p){low={std::min(low.x,p.x),std::min(low.y,p.y),std::min(low.z,p.z)};high={std::max(high.x,p.x),std::max(high.y,p.y),std::max(high.z,p.z)};}
 bool overlaps(const Box &b)const{for(int i=0;i<3;++i)if(high[i]<b.low[i]-epsilon||b.high[i]<low[i]-epsilon)return false;return true;}
};
struct Triangle {std::array<uint32_t,3> i;V a,b,c,n,center;Box box;};
struct Edge {uint32_t lo,hi,face;int sign;bool operator<(const Edge &b)const{return lo==b.lo?hi<b.hi:lo<b.lo;}};
struct Node {Box box;int start=0,end=0,left=-1,right=-1;};
// Non-coplanar intersections return the actual contact, so a legitimate
// shared endpoint/edge can be distinguished from crossing triangle interiors.
bool segmentTriangle(V p,V q,const Triangle &t,V &hit){
 V d=q-p,h=cross(d,t.c-t.a);double det=dot(t.b-t.a,h);
 if(std::abs(det)<1e-18)return false;
 double inv=1/det;V s=p-t.a;double u=dot(s,h)*inv;
 if(u<-epsilon||u>1+epsilon)return false;
 V k=cross(s,t.b-t.a);double v=dot(d,k)*inv;
 if(v<-epsilon||u+v>1+epsilon)return false;
 double length=dot(t.c-t.a,k)*inv;
 if(length<-epsilon||length>1+epsilon)return false;
 hit=p+d*length;return true;
}
struct P {double x,y;};
double side(P a,P b,P p){return (b.x-a.x)*(p.y-a.y)-(b.y-a.y)*(p.x-a.x);}
// Coplanar triangle clipping distinguishes positive-area overlap from a valid
// shared edge. All buffers have a fixed maximum of eight polygon vertices.
bool coplanarInvalidContact(const Triangle &a,const Triangle &b,const std::array<V,3> &shared,int sharedCount){
 int axis=0;for(int i=1;i<3;++i)if(std::abs(a.n[i])>std::abs(a.n[axis]))axis=i;
 auto project=[axis](V p)->P{return {p[(axis+1)%3],p[(axis+2)%3]};};
 std::array<P,8> in{},out{};in[0]=project(a.a);in[1]=project(a.b);in[2]=project(a.c);int count=3;
 std::array<P,3> clip={project(b.a),project(b.b),project(b.c)};
 double orientation=side(clip[0],clip[1],clip[2])>=0?1:-1;
 for(int e=0;e<3&&count;++e){
  int nextCount=0;P first=clip[e],second=clip[(e+1)%3];
  for(int j=0;j<count;++j){
   P p=in[j],q=in[(j+1)%count];double dp=orientation*side(first,second,p),dq=orientation*side(first,second,q);
   bool ip=dp>=0,iq=dq>=0;
   if(ip){if(nextCount==8)throw Error(1,"Invalid coplanar triangle overlap");out[nextCount++]=p;}
   if(ip!=iq){double t=dp/(dp-dq);if(nextCount==8)throw Error(1,"Invalid coplanar triangle overlap");out[nextCount++]={p.x+t*(q.x-p.x),p.y+t*(q.y-p.y)};}
  }
  in=out;count=nextCount;
 }
 double area=0;for(int i=0;i<count;++i)area+=in[i].x*in[(i+1)%count].y-in[i].y*in[(i+1)%count].x;
 if(std::abs(area)*0.5>1e-15)return true;
 if(count==0)return false;
 if(sharedCount==0)return true;
 if(sharedCount==1){P allowed=project(shared[0]);for(int i=0;i<count;++i)if(std::hypot(in[i].x-allowed.x,in[i].y-allowed.y)>1e-8)return true;}
 return false;
}
bool intersection(const Triangle &a,const Triangle &b){
 std::array<V,3> shared{};int n=0;
 const V av[]={a.a,a.b,a.c},bv[]={b.a,b.b,b.c};
 for(int i=0;i<3;++i)for(int j=0;j<3;++j)if(a.i[i]==b.i[j])shared[n++]=av[i];
 if(n==3)return true;
 if(norm(cross(a.n,b.n))<1e-8&&std::abs(dot(a.n,b.a-a.a))<1e-9)
  return coplanarInvalidContact(a,b,shared,n);
 auto legitimate=[&](V p){
  if(n==1)return norm(p-shared[0])<1e-8;
  if(n==2){V d=shared[1]-shared[0];double t=dot(p-shared[0],d)/dot(d,d);return t>=-epsilon&&t<=1+epsilon&&norm(p-(shared[0]+d*t))<1e-8;}
  return false;
 };
 V hit;
 for(int i=0;i<3;++i){
  if(segmentTriangle(av[i],av[(i+1)%3],b,hit)&&!legitimate(hit))return true;
  if(segmentTriangle(bv[i],bv[(i+1)%3],a,hit)&&!legitimate(hit))return true;
 }
 return false;
}
struct Screen {
 SdaHrtfMeshView input;SdaHrtfMeshCancelled cancelled;void *context;SdaHrtfMeshResult reply{};
 std::vector<Triangle> faces;std::vector<uint32_t> parent,order;std::vector<Node> nodes;
 Screen(SdaHrtfMeshView i,SdaHrtfMeshCancelled c,void *p):input(i),cancelled(c),context(p){}
 uint64_t ticks=0,work=0;
 void check(){if((++ticks&1023)==0&&cancelled&&cancelled(context))throw Error(2,"Mesh check cancelled");}
 void charge(){check();if(++work>100000000)throw Error(3,"Mesh verification exceeds local work bound");}
 uint32_t root(uint32_t x){while(parent[x]!=x){parent[x]=parent[parent[x]];x=parent[x];}return x;}
 int tree(int start,int end){
  check();Node node;node.start=start;node.end=end;
  for(int i=start;i<end;++i){node.box.add(faces[order[i]].box.low);node.box.add(faces[order[i]].box.high);}
  int index=int(nodes.size());nodes.push_back(node);
  if(end-start>8){
   V extent=node.box.high-node.box.low;int axis=0;for(int i=1;i<3;++i)if(extent[i]>extent[axis])axis=i;
   int mid=(start+end)/2;
   std::nth_element(order.begin()+start,order.begin()+mid,order.begin()+end,[&](uint32_t a,uint32_t b){return faces[a].center[axis]<faces[b].center[axis];});
   int left=tree(start,mid),right=tree(mid,end);nodes[index].left=left;nodes[index].right=right;
  }
  return index;
 }
 void pair(int x,int y){
  charge();const Node a=nodes[x],b=nodes[y];if(!a.box.overlaps(b.box))return;
  if(x==y){if(a.left>=0){pair(a.left,a.left);pair(a.left,a.right);pair(a.right,a.right);}else{
   for(int i=a.start;i<a.end;++i)for(int j=i+1;j<a.end;++j)test(order[i],order[j]);}return;}
  if(a.left<0&&b.left<0){for(int i=a.start;i<a.end;++i)for(int j=b.start;j<b.end;++j)test(order[i],order[j]);return;}
  if(a.left>=0&&(b.left<0||a.end-a.start>=b.end-b.start)){pair(a.left,y);pair(a.right,y);}else{pair(x,b.left);pair(x,b.right);}
 }
 void test(uint32_t x,uint32_t y){charge();if(faces[x].box.overlaps(faces[y].box)){++reply.intersection_pairs;if(intersection(faces[x],faces[y]))throw Error(1,"Reconstructed head surface intersects itself");}}
 static uint64_t cell(V p){
  int64_t x=int64_t(std::floor(p.x/tolerance))+8192,y=int64_t(std::floor(p.y/tolerance))+8192,z=int64_t(std::floor(p.z/tolerance))+8192;
  if(x<0||x>=16384||y<0||y>=16384||z<0||z>=16384)throw Error(1,"Measured support outside metre coordinate bound");
  return (uint64_t(x)<<28)|(uint64_t(y)<<14)|uint64_t(z);
 }
 void run(){
  if(cancelled&&cancelled(context))throw Error(2,"Mesh check cancelled");
  if(!input.vertices||!input.triangles||!input.support||!input.support_normals||input.vertex_count<4||input.triangle_count<4||input.support_count<4)throw Error(1,"Incomplete measured mesh/support input");
  if(input.vertex_count>80000||input.triangle_count>160000||input.support_count>500000)throw Error(3,"Measured geometry exceeds local processing bound");
  std::vector<Edge> edges;edges.reserve(3*input.triangle_count);faces.reserve(input.triangle_count);
  parent.resize(input.triangle_count);std::iota(parent.begin(),parent.end(),0);
  std::vector<bool> used(input.vertex_count,false);Box bounds;
  for(size_t i=0;i<input.vertex_count;++i){check();V p=load(input.vertices+i*3);if(!finite(p))throw Error(1,"Invalid mesh coordinates");bounds.add(p);}
  V extent=bounds.high-bounds.low;if(std::max({extent.x,extent.y,extent.z})>1||std::min({extent.x,extent.y,extent.z})<0.01)throw Error(1,"Mesh dimensions do not represent measured metres");
  V origin=(bounds.low+bounds.high)*0.5;double volume=0;
  for(size_t f=0;f<input.triangle_count;++f){
   check();Triangle t;
   for(int j=0;j<3;++j){t.i[j]=input.triangles[3*f+j];if(t.i[j]>=input.vertex_count)throw Error(1,"Mesh face index outside vertex buffer");used[t.i[j]]=true;}
   if(t.i[0]==t.i[1]||t.i[1]==t.i[2]||t.i[0]==t.i[2])throw Error(1,"Repeated vertex in head mesh triangle");
   t.a=load(input.vertices+3*t.i[0]);t.b=load(input.vertices+3*t.i[1]);t.c=load(input.vertices+3*t.i[2]);
   V normal=cross(t.b-t.a,t.c-t.a);double magnitude=norm(normal);
   if(magnitude<1e-12)throw Error(1,"Degenerate head mesh triangle");
   t.n=normal*(1/magnitude);t.center=(t.a+t.b+t.c)*(1.0/3);t.box.add(t.a);t.box.add(t.b);t.box.add(t.c);
   reply.surface_area+=magnitude*0.5;volume+=dot(t.a-origin,cross(t.b-origin,t.c-origin))/6;
   reply.maximum_edge=std::max({reply.maximum_edge,norm(t.a-t.b),norm(t.b-t.c),norm(t.c-t.a)});
   if(reply.maximum_edge>maxEdge)throw Error(1,"Head mesh contains unresolved large triangles");
   for(int j=0;j<3;++j){uint32_t a=t.i[j],b=t.i[(j+1)%3];edges.push_back({std::min(a,b),std::max(a,b),uint32_t(f),a<b?1:-1});}
   faces.push_back(t);
  }
  if(std::find(used.begin(),used.end(),false)!=used.end())throw Error(1,"Unreferenced vertices in reconstructed mesh");
  std::sort(edges.begin(),edges.end());
  for(size_t i=0;i<edges.size();){check();size_t end=i+1;while(end<edges.size()&&edges[i].lo==edges[end].lo&&edges[i].hi==edges[end].hi)++end;
   if(end-i!=2)throw Error(1,"Head surface must be a closed two-manifold mesh");
   if(edges[i].sign+edges[i+1].sign!=0)throw Error(1,"Inconsistent mesh face winding");
   uint32_t a=root(edges[i].face),b=root(edges[i+1].face);parent[a]=b;i=end;
  }
  for(size_t i=1;i<faces.size();++i)if(root(uint32_t(i))!=root(0))throw Error(1,"Head mesh contains disconnected surfaces");
  if(volume<=1e-8)throw Error(1,"Head mesh must enclose positive outward-oriented volume");
  reply.enclosed_volume=volume;
  order.resize(faces.size());std::iota(order.begin(),order.end(),0);nodes.reserve(faces.size()/2);tree(0,int(faces.size()));pair(0,0);
  std::unordered_map<uint64_t,int> cells;cells.reserve(input.support_count);
  std::vector<int> next(input.support_count,-1);
  for(size_t i=0;i<input.support_count;++i){check();V p=load(input.support+3*i),n=load(input.support_normals+3*i);
   if(!finite(p)||!finite(n)||norm(n)<1e-6)throw Error(1,"Invalid measured point/normal");
   auto result=cells.emplace(cell(p),int(i));if(!result.second){next[i]=result.first->second;result.first->second=int(i);}
  }
  auto supported=[&](V p,V normal){
   ++reply.support_samples;double best=tolerance*tolerance;
   bool found=false;
   for(int x=-1;x<=1;++x)for(int y=-1;y<=1;++y)for(int z=-1;z<=1;++z){
    charge();auto bucket=cells.find(cell(p+V{double(x),double(y),double(z)}*tolerance));if(bucket==cells.end())continue;
    for(int i=bucket->second;i>=0;i=next[i]){charge();V delta=load(input.support+3*i)-p;double d=dot(delta,delta);
     if(d<=best){V measuredNormal=load(input.support_normals+3*i);if(dot(measuredNormal,normal)>=0.5*norm(measuredNormal)){best=d;found=true;}}
    }
   }
   if(!found)throw Error(1,"Mesh includes a surface without measured depth support");
   reply.support_distance=std::max(reply.support_distance,std::sqrt(best));
  };
  for(const Triangle &t:faces){
   // Sampling every <=1.5 mm along the barycentric lattice also probes large
   // Poisson-filled triangle interiors, rather than checking vertices only.
   int steps=int(std::ceil(std::max({norm(t.a-t.b),norm(t.b-t.c),norm(t.c-t.a)})/tolerance));
   for(int i=0;i<=steps;++i)for(int j=0;j<=steps-i;++j)supported(t.a+(t.b-t.a)*(double(i)/steps)+(t.c-t.a)*(double(j)/steps),t.n);
   supported(t.center,t.n);
  }
  if(cancelled&&cancelled(context))throw Error(2,"Mesh check cancelled");
 }
};
}
extern "C" SdaHrtfMeshResult sda_hrtf_screen_head_mesh(SdaHrtfMeshView input,SdaHrtfMeshCancelled cancelled,void *context){
 Screen screen{input,cancelled,context};
 try{screen.run();std::snprintf(screen.reply.message,sizeof screen.reply.message,"Geometric screen passed; acoustic accuracy and ear landmarks remain unqualified");}
 catch(const Error &e){screen.reply.code=e.code;std::snprintf(screen.reply.message,sizeof screen.reply.message,"%s",e.what());}
 catch(const std::bad_alloc &){screen.reply.code=3;std::snprintf(screen.reply.message,sizeof screen.reply.message,"Insufficient memory for bounded mesh check");}
 catch(...){screen.reply.code=1;std::snprintf(screen.reply.message,sizeof screen.reply.message,"Local mesh check failed");}
 return screen.reply;
}
