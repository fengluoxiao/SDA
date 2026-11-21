// Included inside sda_numcalc after the pinned NumCalc state/types. Only for
// external, rigid, triangular, unreflected, one-DOF ML-FMM source projects.
// Reference path, near integrals, linear solve and postprocessing stay double.
struct QPair { uint16_t loR,loI;int8_t hiR,hiI;
 int real()const{return int(hiR)*65536+loR;}int imag()const{return int(hiI)*65536+loI;}
};
static_assert(sizeof(QPair)==6);
struct PackedDirection { double x,y,z,w; int opposite; };
struct PackedLevel {
 int q=0,half=0,count=0;
 int *elements=nullptr,*neighbors=nullptr,*pairRef=nullptr;
 PackedDirection *dirs=nullptr;
 double *tScale=nullptr,*dScale=nullptr;
 QPair *t=nullptr,*d=nullptr;
};
static bool compact_compatible(){
 const auto &s=state();
 if(s.methodFMM_!=3 || s.numReflectionsOfElements_!=1 || s.isInternalProblem_ || s.Tao_!=1 || std::abs(s.harmonicTimeFactor_)!=1)return false;
 for(int e=0;e<s.numElements_;++e)if(s.listElementProperty[e]!=2 && (s.listElementProperty[e]!=0 || s.listNumberNodesPerElement[e]!=3 || s.ibval[e]!=0))return false;
 for(int l=0;l<s.numClusterLevels_;++l){auto &v=s.clulevarry[l];
  if(v.nClustSLv!=v.nClustOLv || v.nPoinPhiLv!=2*v.nPoinThetLv || v.nPoinSpheLv!=v.nPoinPhiLv*v.nPoinThetLv)return false;
  for(int c=0;c<v.nClustOLv;++c){auto &x=v.ClastArLv[c];if(x.NDOFsPeEl!=1 || x.NumOfEl!=x.NumOfDOFs || x.nuref || x.rffac!=1)return false;}
 }
 return true;
}
static QPair quantize_pair(const Complex &z,double scale){
 if(!scale)return {};
 int r=int(std::lround(z.re()/scale)),i=int(std::lround(z.im()/scale));
 return {uint16_t(r),uint16_t(i),int8_t((r-int(uint16_t(r)))/65536),int8_t((i-int(uint16_t(i)))/65536)};
}
static double pair_scale(const Vector<Complex>& v,int n){
 double peak=0;
 for(int i=0;i<n;++i){const auto &z=v[i];if(!std::isfinite(z.re()) || !std::isfinite(z.im()))throw Failure(1,"Nonfinite far-field coefficient");peak=std::max({peak,std::abs(z.re()),std::abs(z.im())});}
 return peak/8388607.;
}
static void packed_prepare(){
 auto &s=state();auto &rt=*runtime;const int rows=s.numRowsOfCoefficientMatrix_;
 rt.packed=allocate_array<PackedLevel>(s.numClusterLevels_);
 rt.phaseTable=allocate_array<Complex>(65536);
 for(int i=0;i<65536;++i){double a=i*(2.*PI/65536);rt.phaseTable[i].set(std::cos(a),std::sin(a));}
 rt.packedRowScale=allocate_array<double>(rows);std::fill_n(rt.packedRowScale,rows,1.);
 for(int l=0;l<s.numClusterLevels_;++l){auto &v=s.clulevarry[l];auto &p=rt.packed[l];
  const int q=p.q=v.nPoinSpheLv,h=p.half=q/2,n=p.count=v.nClustOLv,ph=v.nPoinPhiLv;
  p.elements=allocate_array<int>(n+1);p.neighbors=allocate_array<int>(n+1);p.elements[0]=p.neighbors[0]=0;
  for(int c=0;c<n;++c){p.elements[c+1]=index_sum(p.elements[c],v.ClastArLv[c].NumOfEl);p.neighbors[c+1]=index_sum(p.neighbors[c],v.ClastArLv[c].NumFanClus);}
  if(rows!=p.elements[n] || p.neighbors[n]%2)throw Failure(1,"Invalid reciprocal cluster topology");
  const int pairs=p.neighbors[n]/2;
  p.t=allocate_array<QPair>(index_product(rows,h));p.tScale=allocate_array<double>(rows);
  p.d=allocate_array<QPair>(index_product(pairs,q));p.dScale=allocate_array<double>(pairs);
  p.dirs=allocate_array<PackedDirection>(q);
  p.pairRef=allocate_array<int>(p.neighbors[n]);if(p.neighbors[n])std::fill_n(p.pairRef,p.neighbors[n],0);
  for(int k=0;k<q;++k){const int other=(v.nPoinThetLv-1-k/ph)*ph+(k%ph+ph/2)%ph;
   const auto *a=v.uvcsphe[k],*b=v.uvcsphe[other];
   if((k<h)==(other<h) || std::abs(a[0]+b[0])+std::abs(a[1]+b[1])+std::abs(a[2]+b[2])>1e-12 || std::abs(v.weisphe[k]-v.weisphe[other])>1e-12)throw Failure(1,"Non-antipodal FMM quadrature");
   p.dirs[k]={a[0],a[1],a[2],v.weisphe[k],other};
  }
  int id=0;
  for(int c=0;c<n;++c){check_now();auto &x=v.ClastArLv[c];
   for(int a=0;a<x.NumFanClus;++a){int neighbor=x.NumsFanClus[a];
    if(neighbor<0 || neighbor>=n || neighbor==c)throw Failure(1,"Invalid far-field neighbor");
    if(neighbor<c)continue;
    auto &y=v.ClastArLv[neighbor];if(!y.NumFanClus)throw Failure(1,"Missing reciprocal neighbor");
    auto *reverse=std::find(y.NumsFanClus,y.NumsFanClus+y.NumFanClus,c);
    if(reverse==y.NumsFanClus+y.NumFanClus || id>=pairs)throw Failure(1,"Missing reciprocal far-field pair");
    int &forward=p.pairRef[p.neighbors[c]+a],&back=p.pairRef[p.neighbors[neighbor]+(reverse-y.NumsFanClus)];
    if(forward || back)throw Failure(1,"Duplicate reciprocal far-field pair");forward=++id;back=-id;
   }
  }
  if(id!=pairs)throw Failure(1,"Incomplete reciprocal far-field matrix");
 }
}
static void packed_store_t(int l,int c,int j,Vector<Complex>& values){
 auto &p=runtime->packed[l];const auto &x=state().clulevarry[l].ClastArLv[c];const int row=state().jelist[x.NumsOfEl[j]][0];
 const double scale=p.tScale[row]=pair_scale(values,p.half);
 for(int k=0;k<p.half;++k)p.t[size_t(p.elements[c])*p.half+size_t(k)*x.NumOfEl+j]=quantize_pair(values[k],scale);
}
static void packed_store_d(int l,int c,int j,Vector<Complex>& values){
 auto &p=runtime->packed[l];int id=p.pairRef[p.neighbors[c]+j]-1;
 if(id<0)throw Failure(1,"Invalid translation pair owner");
 const double scale=p.dScale[id]=pair_scale(values,p.q);
 for(int k=0;k<p.q;++k)p.d[size_t(id)*p.q+k]=quantize_pair(values[k],scale);
}
static void packed_apply(Vector<Complex>* input,Vector<Complex>* output){
 auto &s=state();auto &rt=*runtime;
 for(int l=0;l<s.numClusterLevels_;++l){auto &v=s.clulevarry[l];auto &p=rt.packed[l];
  int q=p.q,h=p.half;
  if(input){
   Vector<Complex> scaled(s.numRowsOfCoefficientMatrix_);
   for(int c=0;c<p.count;++c){check_now();auto &x=v.ClastArLv[c];
    for(int j=0;j<x.NumOfEl;++j){int row=s.jelist[x.NumsOfEl[j]][0];scaled[p.elements[c]+j]=(*input)[row]*p.tScale[row];}
    for(int k=0;k<h;++k){if((k&127)==0)check_now();double r=0,i=0,ar=0,ai=0;
     auto *t=p.t+size_t(p.elements[c])*h+size_t(k)*x.NumOfEl;
     for(int j=0;j<x.NumOfEl;++j){auto &z=scaled[p.elements[c]+j];double a=t[j].real(),b=t[j].imag();
      const double ax=a*z.re(),by=b*z.im(),ay=a*z.im(),bx=b*z.re();r+=ax-by;i+=ay+bx;ar+=-ax-by;ai+=-ay+bx;
     }
     v.zwkT[c*q+k].set(r,i);v.zwkT[c*q+p.dirs[k].opposite].set(ar,ai);
    }
   }
  }
  const Complex *t=input?v.zwkT:s.tmtxlev[l].zTvcLv;
  for(int c=0;c<p.count;++c){check_now();auto &x=v.ClastArLv[c];auto *dst=v.zwkS+c*q;
   for(int k=0;k<q;++k)dst[k].set(0,0);
   for(int j=0;j<x.NumFanClus;++j){int ref=p.pairRef[p.neighbors[c]+j],id=std::abs(ref)-1;
    const auto *d=p.d+size_t(id)*q;const double scale=p.dScale[id];const auto *src=t+x.NumsFanClus[j]*q;
    for(int k=0;k<q;++k){const auto &z=d[ref>0?k:p.dirs[k].opposite];dst[k]+=Complex(z.real()*scale,z.imag()*scale)*src[k];}
   }
   for(int j=0;j<x.NumOfEl;++j){int e=x.NumsOfEl[j],row=s.jelist[e][0];const auto *normal=s.elenor[e];
    Complex sum;auto *pos=s.centel[e];const double k0=s.waveNumbers_*s.harmonicTimeFactor_*65536./(2.*PI);
    const double dx=k0*(pos[0]-x.CoorCent[0]),dy=k0*(pos[1]-x.CoorCent[1]),dz=k0*(pos[2]-x.CoorCent[2]);
    // For a rigid exterior, S(u) = w exp(i k h (x-c).u) (1 - n.u).
    // S(-u) has conjugate phase and multiplier 1+n.u. The ILU row
    // scaling must affect this term just as it does the near coefficients.
    const double scale=rt.packedRowScale[row];
    for(int k=0;k<h;++k){auto &u=p.dirs[k];const double nu=normal[0]*u.x+normal[1]*u.y+normal[2]*u.z;
     const double phase=dx*u.x+dy*u.y+dz*u.z;const int index=int(phase);const double dr=(phase-index)*(2.*PI/65536);
     const auto &base=rt.phaseTable[uint16_t(index)];const Complex z=base*Complex(1.-.5*dr*dr,dr*(1.-dr*dr/6));const double w=u.w*scale;
     sum+=(z*(w*(1.-nu)))*dst[k];sum+=(z.conjugate()*(w*(1.+nu)))*dst[u.opposite];
    }
    if(output)(*output)[row]+=sum;else s.zrhs[row]+=sum;
   }
  }
 }
}
static void packed_release(){
 auto &rt=*runtime;if(!rt.packed)return;
 for(int l=0;l<state().numClusterLevels_;++l){auto &p=rt.packed[l];release_array(p.elements);release_array(p.neighbors);release_array(p.pairRef);
  release_array(p.dirs);release_array(p.tScale);release_array(p.dScale);release_array(p.t);release_array(p.d);}
 release_array(rt.phaseTable);release_array(rt.packedRowScale);release_array(rt.packed);rt.packed=nullptr;
}
