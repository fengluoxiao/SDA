#include <cstdint>
#include <happly.h>
#include <sstream>
#include <cassert>
#include <cstring>
#include <algorithm>
#include <iostream>
static std::string header(const std::string& format,const std::string& type="int") {
 return "ply\nformat "+format+" 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list int "+type+" vertex_indices\nend_header\n";
}
static bool accepts(const std::string& text) {
 try { std::istringstream stream(text,std::ios::in|std::ios::binary);happly::PLYData data(stream);return true; }
 catch(const std::exception&){return false;}
}
template<class T>static void append(std::string& out,T value,bool big) {
 char bytes[sizeof(T)];memcpy(bytes,&value,sizeof(T));if(big)std::reverse(bytes,bytes+sizeof(T));out.append(bytes,sizeof(T));
}
int main() {
 const std::string vertex="0.25 -0.5 0.75\n";
 assert(accepts(header("ascii")+vertex+"3 0 0 0\n"));
 for(const std::string& bad:std::vector<std::string>{"",vertex,"0.25 -0.5\n3 0 0 0\n","bad -0.5 0.75\n3 0 0 0\n",vertex+"3 0 0\n",vertex+"-1 0\n",vertex+"999999999999999999999999\n",vertex+"3 0 invalid 0\n"})assert(!accepts(header("ascii")+bad));
 for(const std::string& type:{"uchar","char"}) {
  assert(accepts(header("ascii",type)+vertex+"3 0 0 0\n"));
  assert(!accepts(header("ascii",type)+vertex+"3 0\n"));
  assert(!accepts(header("ascii",type)+vertex+"3 bad 0 0\n"));
 }
 for(bool big:{false,true}) {
  const std::string format=big?"binary_big_endian":"binary_little_endian";
  std::string body;append(body,0.25f,big);append(body,-0.5f,big);append(body,0.75f,big);append(body,uint32_t(3),big);for(int i=0;i<3;i++)append(body,int32_t(0),big);
  assert(accepts(header(format)+body));
  for(size_t n=0;n<body.size();n++)assert(!accepts(header(format)+body.substr(0,n)));
  std::string zero=body.substr(0,12);append(zero,uint32_t(0),big);assert(accepts(header(format)+zero));
  std::string huge=body.substr(0,12);append(huge,uint32_t(0xffffffff),big);assert(!accepts(header(format)+huge));
 }
 std::cout<<"{\"asciiTruncationRejected\":true,\"binaryTruncationRejected\":true,\"invalidValuesRejected\":true,\"listLengthsBounded\":true,\"emptyListsSafe\":true}"<<std::endl;
}
