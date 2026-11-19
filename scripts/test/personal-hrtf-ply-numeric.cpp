// Include the exact SHA-pinned, patched implementation used by the framework.
#include SDA_PLY_SOURCE
#include <cassert>
#include <limits>
#include <iostream>
int main() {
 const double values[] = {-std::numeric_limits<double>::infinity(),-1e100,-99.5,-0.09,0,0.09,99.5,1e100,std::numeric_limits<double>::infinity(),std::numeric_limits<double>::quiet_NaN()};
 for(double value:values) {
  int i;unsigned int u;long long l;unsigned long long ul;double d;
  get_stored_item(&value,PLY_DOUBLE,i,u,l,ul,d);
  assert((std::isnan(value)&&std::isnan(d))||d==value);
  assert(i==sdaPlyInteger<int>(value) && u==sdaPlyInteger<unsigned int>(value));
  assert(l==sdaPlyInteger<long long>(value));
  if(value<0)assert(u==0);
 }
 double negative=-99.5;int i;unsigned int u;long long l;unsigned long long ul;double d;
 get_stored_item(&negative,PLY_DOUBLE,i,u,l,ul,d);
 assert(d==negative && i==-99 && l==-99 && u==0);
 assert(sdaPlyInteger<long long>(std::ldexp(1.0,63))==std::numeric_limits<long long>::max());
 assert(sdaPlyInteger<unsigned long long>(std::ldexp(1.0,64))==std::numeric_limits<unsigned long long>::max());
 assert(sdaPlyInteger<unsigned int>(1e100)==std::numeric_limits<unsigned int>::max());
 assert(sdaPlyInteger<int>(-1e100)==std::numeric_limits<int>::lowest());
 std::cout<<"{\"negativeFloatPreserved\":true,\"outOfRangeIntegerConversionBounded\":true}"<<std::endl;
}
