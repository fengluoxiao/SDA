#pragma once
#include <vector>
#include <numeric>
#include <cmath>
#include <algorithm>
#include <stdexcept>

// Marching-cubes root fits can leave coincident vertices/micron-long edges.
// Collapse only edges no longer than two micrometres, with a strict 2-micrometre
// cumulative displacement bound. Do not fill holes or simplify measured detail.
// The complete manifold/intersection/support screen still runs afterwards.
template<class Vertices,class Faces>
void sdaCleanPoissonMesh(Vertices& vertices,Faces& faces) {
 const size_t count=vertices.size();
 if(count>1000000 || faces.size()>2000000) throw std::runtime_error("Reconstructed mesh exceeds bounded on-device measurement capacity");
 std::vector<size_t> parent(count);std::iota(parent.begin(),parent.end(),0);
 std::vector<double> displacement(count,0);
 auto root=[&](size_t i){size_t r=i;while(parent[r]!=r)r=parent[r];while(parent[i]!=i){size_t next=parent[i];parent[i]=r;i=next;}return r;};
 auto distance=[&](size_t a,size_t b){double sum=0;for(int k=0;k<3;k++){double d=double(vertices[a].position[k])-double(vertices[b].position[k]);sum+=d*d;}return std::sqrt(sum);};
 for(int pass=0;pass<4;pass++){
  bool changed=false;
  for(auto &face:faces){
   if(face.size()!=3)throw std::runtime_error("Poisson emitted non-triangular surface");
   size_t ids[3];for(int k=0;k<3;k++){if(size_t(face[k])>=count)throw std::runtime_error("Poisson emitted invalid indices");ids[k]=root(size_t(face[k]));}
   if(ids[0]==ids[1]||ids[1]==ids[2]||ids[0]==ids[2])continue;
   int shortest=0;double length=distance(ids[0],ids[1]);
   for(int k=1;k<3;k++){double d=distance(ids[k],ids[(k+1)%3]);if(d<length){length=d;shortest=k;}}
   if(length>0.000002)continue;
   size_t a=ids[shortest],b=ids[(shortest+1)%3];
   if(length+displacement[b]<=0.000002){parent[b]=a;displacement[a]=std::max(displacement[a],length+displacement[b]);changed=true;}
   else if(length+displacement[a]<=0.000002){parent[a]=b;displacement[b]=std::max(displacement[b],length+displacement[a]);changed=true;}
  }
  if(!changed)break;
 }
 size_t kept=0;
 std::vector<bool> used(count,false);
 for(auto face:faces){
  for(int k=0;k<3;k++)face[k]=root(size_t(face[k]));
  if(face[0]==face[1]||face[1]==face[2]||face[0]==face[2])continue;
  for(int k=0;k<3;k++)used[size_t(face[k])]=true;
  faces[kept++]=std::move(face);
 }
 faces.resize(kept);
 std::vector<size_t> mapping(count);kept=0;
 for(size_t i=0;i<count;i++)if(used[i]){mapping[i]=kept;if(kept!=i)vertices[kept]=std::move(vertices[i]);kept++;}
 vertices.resize(kept);
 for(auto &face:faces)for(int k=0;k<3;k++)face[k]=mapping[size_t(face[k])];
 if(vertices.size()<4||faces.size()<4)throw std::runtime_error("Poisson emitted no usable surface");
}
