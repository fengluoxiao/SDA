#include "SdaTrackingPrediction.hpp"
#include <cassert>
#include <limits>
#include <cstdio>
int main() {
 using M=Eigen::Matrix4f; using V=Eigen::Vector3f;
 auto pose=[](float angle) { M m=M::Identity();m.block<3,3>(0,0)=Eigen::AngleAxisf(angle,V::UnitY()).toRotationMatrix();m.block<3,1>(0,3)=V(0,0,-.4f)+m.block<3,3>(0,0)*V(0,0,.4f);return m; };
 for(int sign : {-1,1})for(int fps : {10,15,30,60})for(int step=1;step<90;step++) {
  const float a=sign*step*.01f,b=sign*(step+1)*.01f,c=sign*(step+2)*.01f;
  M p; assert(sdaPredictTrackingPose(pose(a),pose(b),1,1+1.0/fps,1+2.0/fps,p));
  assert((p.block<3,1>(0,3)-pose(c).block<3,1>(0,3)).norm()<0.00005f);
  assert(Eigen::AngleAxisf(p.block<3,3>(0,0).transpose()*pose(c).block<3,3>(0,0)).angle()<1e-5f);
 }
 M a=M::Identity(),p;assert(sdaPredictTrackingPose(a,a,1,1.05,1.10,p));assert(p.isApprox(a));
 for(double t : {1.0,1.05,1.30,std::numeric_limits<double>::quiet_NaN()}) assert(!sdaPredictTrackingPose(a,a,1,1.05,t,p));
 M bad=a;bad(0,0)=1.1f;assert(!sdaPredictTrackingPose(bad,a,1,1.05,1.10,p));
 bad=a;bad(0,3)=.2f;assert(!sdaPredictTrackingPose(a,bad,1,1.05,1.10,p));
 bad=a;bad.block<3,3>(0,0)=Eigen::AngleAxisf(.2f,V::UnitY()).toRotationMatrix();assert(!sdaPredictTrackingPose(a,bad,1,1.05,1.10,p));
 printf("PASS: bounded bilateral camera-orbit prediction; stationary, stale, clock, scale and jump rejection\n");
}
