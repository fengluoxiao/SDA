#include <PoissonRecon/ExecuteEntryFunctions.hpp>
#include <cstdio>
#include <cmath>
#include <cstdlib>
#include <fstream>
#include <iostream>
int main(int argc,char**argv){
 int count=argc>1?atoi(argv[1]):100000;
 FILE* f=fopen("synthetic-support.ply","w");
 fprintf(f,"ply\nformat ascii 1.0\ncomment StandardCyborgFusionVersion 1.0.0\nelement vertex %d\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nproperty float surfel_radius\nelement face 0\nproperty list uchar int vertex_indices\nend_header\n",count);
 for(int i=0;i<count;i++){
  double y=1-2*(i+0.5)/count,r=sqrt(1-y*y),phi=i*2.399963229728653,x=cos(phi)*r,z=sin(phi)*r;
  double nx=x/.09,ny=y/.12,nz=z/.1,n=sqrt(nx*nx+ny*ny+nz*nz);
  fprintf(f,"%.9g %.9g %.9g %.9g %.9g %.9g 128 128 128 %f\n",x*.09,y*.12,z*.1,nx/n*.001,ny/n*.001,nz/n*.001,.001);
 }
 fclose(f);
 PoissonReconParameters parameters;parameters.Depth=9;parameters.SamplesPerNode=1;
 PoissonReconExecute("support.ply","synthetic-candidate.ply",true,parameters,[](float v){std::cout<<"progress "<<v<<std::endl;return true;});
 std::ifstream in("output.ply");if(!in){std::cerr<<"NO OUTPUT"<<std::endl;return 1;}
 std::string line;while(std::getline(in,line)){std::cout<<line<<std::endl;if(line=="end_header")break;}
}
