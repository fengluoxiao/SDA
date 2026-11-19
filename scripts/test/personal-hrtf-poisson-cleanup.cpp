#include "SdaPoissonMeshCleanup.hpp"
#include <array>
#include <iostream>
struct Vertex { std::array<float,3> position; };
int main(){
 auto require=[](bool ok){if(!ok)throw std::runtime_error("Bounded Poisson cleanup regression");};
 std::vector<Vertex> v={{{0,0,0}},{{.1f,0,0}},{{0,.1f,0}},{{0,0,.1f}},{{.0000001f,0,0}}};
 std::vector<std::vector<unsigned>> f={{0,2,1},{0,1,3},{0,3,2},{1,2,3},{0,4,1}};
 sdaCleanPoissonMesh(v,f);require(v.size()==4&&f.size()==4);
 for(const auto&face:f)for(auto i:face)require(i<v.size());
 std::vector<Vertex> larger={{{0,0,0}},{{.1f,0,0}},{{0,.1f,0}},{{0,0,.1f}},{{.000004f,0,0}}};
 std::vector<std::vector<unsigned>> g={{0,2,1},{0,1,3},{0,3,2},{1,2,3},{0,4,1}};
 sdaCleanPoissonMesh(larger,g);require(larger.size()==5&&g.size()==5); // no invisible relaxation beyond 2 um
 bool rejected=false;try { std::vector<Vertex> empty;std::vector<std::vector<unsigned>> faces;sdaCleanPoissonMesh(empty,faces); } catch(const std::runtime_error&){rejected=true;}require(rejected);
 rejected=false;try { std::vector<Vertex> points(4);std::vector<std::vector<unsigned>> faces={{0,1,99}};sdaCleanPoissonMesh(points,faces); }catch(const std::runtime_error&){rejected=true;}require(rejected);
 std::cout<<"{\"boundedMicroscopicCleanupPassed\":true,\"largerEdgesPreserved\":true,\"invalidInputRejected\":true}"<<std::endl;
}
