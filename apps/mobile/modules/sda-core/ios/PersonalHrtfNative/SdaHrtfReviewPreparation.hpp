// Private implementation included after Build. No artificial vertex is ever
// returned to the review/export UI or passed to the depth support index.
struct ReviewPreparation {
 using Cell=std::array<int,3>;
 static Cell cell(V p){return {int(std::floor(p[0]/.006)),int(std::floor(p[1]/.006)),int(std::floor(p[2]/.006))};}
 SdaHrtfProjectInput input;SdaHrtfMeshCancelled cancelled;void *context;
 std::vector<float> vertices;std::vector<uint32_t> faces,leftFaces,rightFaces;
 V front{},left{},up{},origin{},ears[2]{};
 using Tri=std::array<uint32_t,3>;
 void check(){if(cancelled&&cancelled(context))throw Failure(2,"Acoustic mesh preparation cancelled");}
 static V point(const float *v,uint32_t i){return {v[3*i],v[3*i+1],v[3*i+2]};}
 uint32_t append(V v){if(vertices.size()/3>=20000)throw Failure(3,"Prepared vertices exceed solver capacity");auto id=uint32_t(vertices.size()/3);for(auto x:v)vertices.push_back(float(x));return id;}
 static Tri key(Tri t){std::rotate(t.begin(),std::min_element(t.begin(),t.end()),t.end());return t;}
 static void face(std::vector<uint32_t>& indices,uint32_t a,uint32_t b,uint32_t c){if(indices.size()>=60000)throw Failure(3,"Prepared faces exceed solver capacity");indices.insert(indices.end(),{a,b,c});}
 using Ring=std::vector<uint32_t>;
 using Rings=std::vector<Ring>;
 Rings boundaries(const uint32_t *indices,size_t count,SdaHrtfReviewProjectResult *diagnostic=nullptr){
  std::vector<uint64_t> edges;edges.reserve(count*3);
  for(size_t f=0;f<count;++f){if(f%512==0)check();for(int j=0;j<3;++j){uint32_t a=indices[3*f+j],b=indices[3*f+(j+1)%3];
   require(a!=b,"Degenerate review face");edges.push_back((uint64_t(std::min(a,b))<<32)|(uint64_t(std::max(a,b))<<1)|(a>b));}}
  std::sort(edges.begin(),edges.end());std::map<uint32_t,uint32_t> next;
  for(size_t i=0;i<edges.size();){if(i%1024==0)check();auto e=edges[i];size_t end=i+1;while(end<edges.size() && edges[end]/2==e/2)++end;
   if(end-i==1){uint32_t a=uint32_t(e>>32),b=(uint32_t(e)>>1);if(e&1)std::swap(a,b);
    require(next.emplace(a,b).second,"Branched review boundary");
   }else require(end-i==2 && edges[i]!=edges[i+1],"Nonmanifold or reversed review triangles");i=end;
  }
  Rings rings;
  while(!next.empty()){
   if(diagnostic)diagnostic->review_loops=rings.size()+1;
   if(rings.size()>=65){if(diagnostic)diagnostic->boundary_issue="too-many-contours";throw Failure(3,"Too many separate review boundaries");}
   Ring ring;auto at=next.begin()->first,first=at;
   do{if(ring.size()%512==0)check();ring.push_back(at);auto it=next.find(at);require(it!=next.end(),"Open review boundary chain");at=it->second;next.erase(it);}while(at!=first);
   require(ring.size()>=3,"Degenerate review boundary");rings.emplace_back(std::move(ring));
  }
  return rings;
 }
 bool posterior(V p)const{return dot(sub(p,origin),front)<-.018 && std::min(norm(sub(p,ears[0])),norm(sub(p,ears[1])))>.035;}
 const char *region(const Ring &ring,const float *v)const{
  bool l=false,r=false,back=true;
  for(auto id:ring){auto p=point(v,id);l|=norm(sub(p,ears[0]))<=.035;r|=norm(sub(p,ears[1]))<=.035;back&=posterior(p);}
  return l&&r?"both-ears":l?"left-ear":r?"right-ear":back?"posterior":"front-or-side";
 }
 struct Gaps {std::vector<uint32_t> triangles;bool hasPosterior=false;};
 Gaps classify(const Rings &rings,SdaHrtfMeshView m,const std::vector<unsigned char> &sourceLocks,SdaHrtfReviewProjectResult &result){
  Gaps gaps;std::vector<const char*> regions;std::vector<uint8_t> nearEar;double totalArea=0;
  auto fail=[&](const char *issue,const char *where){result.boundary_issue=issue;result.boundary_region=where;throw Failure(1,issue);};
  for(const auto &ring:rings){
   check();const auto where=region(ring,m.vertices);V lo=point(m.vertices,ring[0]),hi=lo;bool back=true,ear=false,source=false;
   for(auto id:ring){auto p=point(m.vertices,id);back&=posterior(p);auto d=std::min(norm(sub(p,ears[0])),norm(sub(p,ears[1])));
    ear|=d<=.035;source|=sourceLocks[id] || d<.006;for(int j=0;j<3;++j){lo[j]=std::min(lo[j],p[j]);hi[j]=std::max(hi[j],p[j]);}}
   if(source)fail("confirmed-source-gap",where);
   if(norm(sub(hi,lo))>(ear?.004:.008) || ring.size()>12){
    if(!back)fail("open-anatomical-contour",where);
    // A local posterior opening must not become a fictitious head-sized baffle.
    double low=1e20,high=-1e20;for(auto id:ring){double y=dot(sub(point(m.vertices,id),origin),left);low=std::min(low,y);high=std::max(high,y);}
    if(low>=0 || high<=0 || high-low<.45*norm(sub(ears[0],ears[1])) || gaps.hasPosterior)fail("ambiguous-posterior-contours",where);
    gaps.hasPosterior=true;continue;
   }
   // Only locally convex, nearly planar holes. Opposite edge winding closes
   // the review triangles, without adding vertices or constructing anatomy.
   Ring ids(ring.rbegin(),ring.rend());V a=point(m.vertices,ids[0]),normal{};
   for(size_t i=1;i+1<ids.size();++i)normal=add(normal,cross(sub(point(m.vertices,ids[i]),a),sub(point(m.vertices,ids[i+1]),a)));
   double area=.5*norm(normal);totalArea+=area;
   if(!(area>5e-13 && area<=(ear?6e-6:32e-6)) || totalArea>96e-6 || nearEar.size()+ids.size()-2>128)fail("gap-outside-repair-limit",where);
   normal=mul(normal,.5/area);
   for(size_t i=0;i<ids.size();++i){V b=point(m.vertices,ids[i]),edge=sub(point(m.vertices,ids[(i+1)%ids.size()]),b);
    if(std::abs(dot(sub(b,a),normal))>(ear?.00025:.00045))fail("nonplanar-gap",where);
    for(size_t j=2;j<ids.size();++j)if(dot(cross(edge,sub(point(m.vertices,ids[(i+j)%ids.size()]),b)),normal)<=1e-12)fail("nonplanar-gap",where);
   }
   for(size_t i=1;i+1<ids.size();++i){gaps.triangles.insert(gaps.triangles.end(),{ids[0],ids[i],ids[i+1]});nearEar.push_back(ear);regions.push_back(where);}
  }
  if(!nearEar.empty()){
   size_t failed=0;auto r=sda_check_gap_support(m,gaps.triangles.data(),nearEar.data(),nearEar.size(),failed,cancelled,context);
   if(r.code==1 && failed<regions.size())fail("insufficient-interior-depth",regions[failed]);
   if(r.code)throw Failure(r.code,r.message);
  }
  result.retriangulated_faces=nearEar.size();return gaps;
 }
 void refine(){
  for(int iteration=0;iteration<12;++iteration){check();std::map<std::pair<uint32_t,uint32_t>,uint32_t> mid;
   for(size_t i=0;i<faces.size();i+=3){for(int j=0;j<3;++j){auto a=faces[i+j],b=faces[i+(j+1)%3];
    if(norm(sub(point(vertices.data(),a),point(vertices.data(),b)))>.010){auto edge=std::minmax(a,b);if(!mid.count(edge)){V p=mul(add(point(vertices.data(),a),point(vertices.data(),b)),.5);mid[edge]=append(p);}}}}
   if(mid.empty())return;
   std::set<uint32_t> locked(leftFaces.begin(),leftFaces.end());locked.insert(rightFaces.begin(),rightFaces.end());
   std::vector<uint32_t> out,remap(faces.size()/3);
   for(size_t f=0;f<remap.size();++f){if(f%512==0)check();remap[f]=uint32_t(out.size()/3);Tri t{faces[f*3],faces[f*3+1],faces[f*3+2]},m{};int cuts=0;
    for(int j=0;j<3;++j){auto it=mid.find(std::minmax(t[j],t[(j+1)%3]));m[j]=it==mid.end()?UINT32_MAX:it->second;cuts+=m[j]!=UINT32_MAX;}
    require(!cuts || !locked.count(uint32_t(f)),"Confirmed ear face is too coarse for acoustic preparation");
    if(cuts==0)face(out,t[0],t[1],t[2]);else if(cuts==3){for(int j=0;j<3;++j)face(out,t[j],m[j],m[(j+2)%3]);face(out,m[0],m[1],m[2]);}
    else{int j=0;while((cuts==1)!=(m[j]!=UINT32_MAX))++j;int b=(j+1)%3,c=(j+2)%3;
     if(cuts==1){face(out,t[j],m[j],t[c]);face(out,m[j],t[b],t[c]);}
     else{face(out,t[c],m[c],m[b]);face(out,t[j],t[b],m[b]);face(out,t[j],m[b],m[c]);}}
   }
   for(auto *patch:{&leftFaces,&rightFaces})for(auto &id:*patch)id=remap[id];faces.swap(out);
  }throw Failure(3,"Review refinement exceeds local work limit");
 }
 void closePosterior(const std::vector<uint32_t>& loop){
  if(loop.empty())return;
  // A ruled computational boundary, not a reconstructed skull. Only a single
  // star-shaped posterior contour can use this path. Winding is not repaired.
  V center{};double minX=1e20;for(auto i:loop){auto v=point(vertices.data(),i);center=add(center,v);minX=std::min(minX,dot(v,front));}
  center=mul(center,1./loop.size());center=add(center,mul(front,minX-.003-dot(center,front)));
  double angle=0,radius=0,perimeter=0;
  for(size_t j=0;j<loop.size();++j){V a=sub(point(vertices.data(),loop[j]),center),b=sub(point(vertices.data(),loop[(j+1)%loop.size()]),center);
   double ax=dot(a,left),ay=dot(a,up),bx=dot(b,left),by=dot(b,up),turn=std::atan2(ax*by-ay*bx,ax*bx+ay*by);
   require(turn>1e-9 && turn<pi/2,"Posterior contour does not bound a simple radial disk");angle+=turn;radius=std::max(radius,norm(a));perimeter+=norm(sub(a,b));}
  require(std::abs(angle-2*pi)<1e-5,"Posterior contour winds more than once");
  // Arc length, not boundary vertex number: dense patches must not produce
  // long or crossing diagonals. The outer loop itself is never moved.
  std::vector<double> arc{0};for(size_t j=0;j<loop.size();++j)arc.push_back(arc.back()+norm(sub(point(vertices.data(),loop[j]),point(vertices.data(),loop[(j+1)%loop.size()])))/perimeter);
  std::vector<uint32_t> outer=loop;auto outerArc=arc;int layers=std::max(1,int(std::ceil(radius/.0045)));
  for(int layer=layers-1;layer>=1;--layer){check();double t=double(layer)/layers;size_t n=std::max<size_t>(4,size_t(std::ceil(perimeter*t/.0045)));
   std::vector<uint32_t> inner;std::vector<double> next;for(size_t j=0;j<n;++j){double a=double(j)/n;auto k=size_t(std::upper_bound(arc.begin(),arc.end(),a)-arc.begin()-1);double f=(a-arc[k])/(arc[k+1]-arc[k]);
    V p=add(mul(point(vertices.data(),loop[k]),1-f),mul(point(vertices.data(),loop[(k+1)%loop.size()]),f));inner.push_back(append(add(center,mul(sub(p,center),t))));next.push_back(a);}next.push_back(1);
   size_t a=0,b=0;while(a<outer.size() || b<inner.size()){
    if(a<outer.size() && (b==inner.size() || outerArc[a+1]<=next[b+1])){face(faces,outer[(a+1)%outer.size()],outer[a],inner[b%inner.size()]);++a;}
    else{face(faces,outer[a%outer.size()],inner[b%inner.size()],inner[(b+1)%inner.size()]);++b;}}
   outer.swap(inner);outerArc.swap(next);
  }
  auto tip=append(center);for(size_t j=0;j<outer.size();++j)face(faces,outer[(j+1)%outer.size()],outer[j],tip);
 }
 bool prepare(const char *root,uint64_t room,uint64_t physical,SdaHrtfReviewProjectResult &result,std::set<Cell>& keep,int pass){
  check();auto original=input.mesh;
  require(original.vertices && original.triangles && original.support && original.support_normals && original.vertex_count>=4 && original.vertex_count<=1000000 && original.triangle_count>=4 && original.triangle_count<=2000000 && original.support_count>=4 && original.support_count<=SDA_HRTF_MAXIMUM_SUPPORT_POINTS,"Invalid review mesh buffers");
  uint64_t estimate=32*MiB+original.vertex_count*192ull+original.triangle_count*128ull+original.support_count*32ull;
  if(estimate>sda_hrtf_recommended_budget(room,physical))throw Failure(3,"Insufficient headroom to prepare the measured surface");
  for(size_t j=0;j<original.vertex_count*3;++j){if(j%4096==0)check();require(std::isfinite(original.vertices[j]) && std::abs(original.vertices[j])<10,"Invalid measured vertex");}
  for(size_t j=0;j<original.triangle_count*3;++j){if(j%4096==0)check();require(original.triangles[j]<original.vertex_count,"Invalid review face index");}
  std::copy_n(input.front,3,front.begin());std::copy_n(input.up,3,up.begin());require(std::abs(norm(front)-1)<1e-6 && std::abs(norm(up)-1)<1e-6 && std::abs(dot(front,up))<1e-6,"Invalid anatomical axes");left=cross(up,front);
  Build raw{input,cancelled,context};ears[0]=raw.patch(input.left_faces,input.left_face_count).center;ears[1]=raw.patch(input.right_faces,input.right_face_count).center;origin=mul(add(ears[0],ears[1]),.5);
  std::map<Tri,int> patches;std::vector<unsigned char> locks(original.vertex_count);
  for(int e=0;e<2;++e){auto ids=e?input.right_faces:input.left_faces;auto n=e?input.right_face_count:input.left_face_count;
   for(size_t j=0;j<n;++j){const auto *t=original.triangles+3*ids[j];require(patches.emplace(key({t[0],t[1],t[2]}),e).second,"Overlapping ear sources");for(int k=0;k<3;++k)locks[t[k]]=1;}}
  // Establish anatomical left/right before attributing a failure to one ear.
  V separation=sub(ears[0],ears[1]);require(dot(separation,left)>.8*norm(separation),"Confirmed ears disagree with left/right head frame");
  auto rings=boundaries(original.triangles,original.triangle_count,&result);
  auto gaps=classify(rings,original,locks,result);
  std::vector<uint32_t> repaired;std::set<Tri> gapFaces;
  if(!gaps.triangles.empty()){
   repaired.assign(original.triangles,original.triangles+3*original.triangle_count);repaired.insert(repaired.end(),gaps.triangles.begin(),gaps.triangles.end());
   for(size_t j=0;j<gaps.triangles.size();j+=3){auto *t=gaps.triangles.data()+j;gapFaces.insert(key({t[0],t[1],t[2]}));for(int k=0;k<3;++k)locks[t[k]]=1;}
   original.triangles=repaired.data();original.triangle_count=repaired.size()/3;
  }
  std::vector<uint32_t> simple(original.triangle_count*3);
  constexpr size_t target=11500;
  if(original.triangle_count>target){float error=0;std::vector<float> normals(original.vertex_count*3);double nose=-1e20;
   for(size_t v=0;v<original.vertex_count;++v)nose=std::max(nose,dot(front,point(original.vertices,uint32_t(v))));
   for(size_t i=0;i<original.triangle_count;++i){if(i%1024==0)check();const auto *t=original.triangles+i*3;
    V a=point(original.vertices,t[0]),n=cross(sub(point(original.vertices,t[1]),a),sub(point(original.vertices,t[2]),a));
    for(int j=0;j<3;++j)for(int k=0;k<3;++k)normals[3*t[j]+k]+=float(n[k]);}
   for(size_t i=0;i<original.vertex_count;++i){V n=point(normals.data(),uint32_t(i));double size=norm(n);require(size>1e-13,"Unresolved review vertex normal");for(int j=0;j<3;++j)normals[i*3+j]=float(n[j]/size);
    const auto p=point(original.vertices,uint32_t(i));if(dot(front,p)>nose-.001 || keep.count(cell(p)))locks[i]=1;}
   const float weights[]={.05f,.05f,.05f};
   for(int pass=0;;++pass){check();size_t size=meshopt_simplifyWithAttributes(simple.data(),original.triangles,original.triangle_count*3,original.vertices,original.vertex_count,3*sizeof(float),normals.data(),12,weights,3,locks.data(),target*3,.00035f,meshopt_SimplifyErrorAbsolute,&error);
    require(std::isfinite(error) && error<=.000351f,"Measured surface simplification exceeds tolerance");
    std::vector<uint8_t> protect(original.vertex_count);bool retry=false;
    for(size_t f=0;f<size;f+=3){auto *t=simple.data()+f;V a=point(original.vertices,t[0]);V n=cross(sub(point(original.vertices,t[1]),a),sub(point(original.vertices,t[2]),a));
     V ref=add(add(point(normals.data(),t[0]),point(normals.data(),t[1])),point(normals.data(),t[2]));
     if(dot(n,ref)<.85*norm(n)*norm(ref) && (!locks[t[0]] || !locks[t[1]] || !locks[t[2]])){for(int k=0;k<3;++k)protect[t[k]]=1;retry=true;}}
    if(!retry){simple.resize(size);break;}
    if(pass>=4)throw Failure(1,"Measured folds cannot be simplified at the required normal tolerance");
    for(size_t f=0;f<original.triangle_count;++f){auto *t=original.triangles+3*f;if(protect[t[0]] || protect[t[1]] || protect[t[2]])for(int k=0;k<3;++k)locks[t[k]]=1;}
   }
  }else std::copy_n(original.triangles,simple.size(),simple.begin());
  if(simple.size()>60000)throw Failure(3,"Measured folds cannot fit the solver face limit at the required tolerance");
  std::vector<uint32_t> remap(original.vertex_count,UINT32_MAX);
  for(size_t j=0;j<simple.size();j+=3){if(j%1536==0)check();auto match=patches.find(key({simple[j],simple[j+1],simple[j+2]}));
   if(!gapFaces.empty())gapFaces.erase(key({simple[j],simple[j+1],simple[j+2]}));
   if(match!=patches.end()){(match->second?rightFaces:leftFaces).push_back(uint32_t(j/3));patches.erase(match);}
   for(int k=0;k<3;++k){auto id=simple[j+k];if(remap[id]==UINT32_MAX)remap[id]=append(point(original.vertices,id));faces.push_back(remap[id]);}}
  require(patches.empty() && gapFaces.empty(),"Simplification changed a confirmed source or depth-backed gap face");refine();auto remaining=boundaries(faces.data(),faces.size()/3);
  require(remaining.size()==size_t(gaps.hasPosterior),"Simplification changed surface topology");
  Ring loop;if(!remaining.empty()){loop=std::move(remaining.front());for(auto id:loop)require(posterior(point(vertices.data(),id)),"Simplified posterior boundary reaches measured anatomy");}
  if(original.triangle_count>target){
   // Collapses of nearly flat or skinny faces need not be identical on x86
   // and ARM. Only independently supported geometry is admissible on either.
   SdaHrtfMeshView partial=original;partial.vertices=vertices.data();partial.vertex_count=vertices.size()/3;partial.triangles=faces.data();partial.triangle_count=faces.size()/3;
   std::vector<uint8_t> mask(partial.triangle_count);auto selected=sda_hrtf_select_measured_faces(partial,mask.data(),mask.size(),cancelled,context);
   if(selected.code)throw Failure(selected.code,selected.message);
   size_t before=keep.size();bool bad=false;
   for(size_t f=0;f<mask.size();++f)if(!mask[f]){bad=true;
    for(int k=0;k<3;++k){const auto c=cell(point(partial.vertices,faces[3*f+k]));
     for(int x=-1;x<=1;++x)for(int y=-1;y<=1;++y)for(int z=-1;z<=1;++z)keep.insert({c[0]+x,c[1]+y,c[2]+z});}}
   if(bad){if(pass>=2 || keep.size()==before)throw Failure(1,"Cannot simplify the surface without losing measured depth support");return false;}
  }
  result.measured_faces=faces.size()/3;closePosterior(loop);result.boundary_faces=faces.size()/3-result.measured_faces;
  result.plan=sda_hrtf_sampling_plan(room,physical,vertices.size()/3,faces.size()/3);if(!result.plan.tier)throw Failure(3,"Prepared surface leaves no room for the evaluation grid");
  input.mesh.vertices=vertices.data();input.mesh.vertex_count=vertices.size()/3;input.mesh.triangles=faces.data();input.mesh.triangle_count=faces.size()/3;
  input.left_faces=leftFaces.data();input.right_faces=rightFaces.data();input.left_face_count=leftFaces.size();input.right_face_count=rightFaces.size();
  input.bins=result.plan.bins;input.azimuth_steps=result.plan.azimuth_steps;input.elevation_steps=result.plan.elevation_steps;input.reference_radius=1.2;
  Build project{input,cancelled,context};result.project=project.run(root,&original,result.measured_faces,&result.review_deviation,result.retriangulated_faces);return true;
 }
};
