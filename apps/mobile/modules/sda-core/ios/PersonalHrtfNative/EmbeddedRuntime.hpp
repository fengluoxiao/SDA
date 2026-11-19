#pragma once
#include "SdaHrtfSolver.h"
#include <algorithm>
#include <array>
#include <atomic>
#include <cassert>
#include <cerrno>
#include <cmath>
#include <complex>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iomanip>
#include <iostream>
#include <limits>
#include <list>
#include <memory>
#include <mutex>
#include <new>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>
struct SdaHrtfSolver {
 std::filesystem::path root;
 uint64_t budget;
 std::atomic<bool> cancelled{false};
 std::mutex mutex;
};
namespace sda_numcalc {
struct Failure : std::runtime_error {
 int code;
 Failure(int c, const std::string &s):std::runtime_error(s),code(c){}
};
struct NullBuffer:std::streambuf { int overflow(int c) override {return traits_type::not_eof(c);} };
struct Allocation {uint64_t bytes; void (*dispose)(void *); std::list<void*>::iterator order;};
struct Runtime {
 SdaHrtfSolver &owner;
 std::filesystem::path work;
 uint64_t used=0,peak=0,ticks=0;
 std::unordered_map<void*,Allocation> arrays;
 std::list<void*> order;
 std::set<FILE*> files;
 SdaHrtfProgress progress=nullptr;
 void *callbackContext=nullptr;
 std::tm wallTime{};
 NullBuffer nullBuffer;
 std::ostream quiet{&nullBuffer};
 Runtime(SdaHrtfSolver &o,const std::filesystem::path &w):owner(o),work(w){}
 void clear() noexcept;
 ~Runtime(){clear();}
};
extern thread_local Runtime *runtime;
inline void check_now() {
 if (!runtime) throw Failure(1,"Missing native solver context");
 if (runtime->owner.cancelled.load(std::memory_order_relaxed)) throw Failure(2,"Calculation cancelled");
}
inline void checkpoint() {if ((++runtime->ticks & 1023)==0) check_now();}
template<class T> T *allocate_array(size_t count) {
 check_now();
 // Upstream allocates zero-length incident-source arrays when a reciprocal
 // ear source is used. They have no elements and must not enter the ownership
 // registry: optimized Apple new[] may fold these empty allocations. Keeping
 // an order entry for an already-present address corrupts cleanup bookkeeping.
 if(count==0)return nullptr;
 if(count>uint64_t(std::numeric_limits<int>::max()) || count>runtime->owner.budget/sizeof(T))
  throw Failure(3,"Native solver array exceeds memory budget");
 // Charge bounded bookkeeping as well; never retain an ever-growing log of
 // arrays that have already been freed during the many frequency solves.
 uint64_t bytes=count*sizeof(T)+128;
 if(bytes>runtime->owner.budget-runtime->used) throw Failure(3,"Native solver reached memory budget");
 runtime->used+=bytes;runtime->peak=std::max(runtime->peak,runtime->used);
 T *p=nullptr;bool linked=false;
 try {
  p=new T[count];
  runtime->order.push_back(p);linked=true;
  auto inserted=runtime->arrays.emplace(p,Allocation{bytes,[](void *q){delete[] static_cast<T*>(q);},std::prev(runtime->order.end())});
  if(!inserted.second)throw Failure(1,"Duplicate live solver allocation");
 } catch(...) {
  if(linked)runtime->order.pop_back();
  runtime->used-=bytes;delete[] p;throw;
 }
 return p;
}
template<class T> void release_array_checked(T *p, const char *file, int line, const char *expression) {
 if(!p) return;
 auto found=runtime->arrays.find(p);
 if(found==runtime->arrays.end()) {
  std::fprintf(stderr,"Untracked solver array %s at %s:%d\n",expression,file,line);
  throw Failure(1,std::string("Untracked solver array: ")+expression+" at "+file+":"+std::to_string(line));
 }
 auto a=found->second;runtime->order.erase(a.order);runtime->arrays.erase(found);runtime->used-=a.bytes;a.dispose(p);
}
#define release_array(p) release_array_checked((p),__FILE__,__LINE__,#p)
inline void Runtime::clear() noexcept {
 // Only live arrays are recorded; cancellation/error cleanup is bounded by
 // current allocations, independent of elapsed frequencies or iterations.
 while(!order.empty()) {
  void *value=order.back();auto found=arrays.find(value);
  if(found==arrays.end()){std::fprintf(stderr,"Native cleanup invariant failed: live map=%zu order=%zu bytes=%llu\n",arrays.size(),order.size(),(unsigned long long)used);std::terminate();}
  auto a=found->second;order.pop_back();arrays.erase(found);used-=a.bytes;a.dispose(value);
 }
 for(FILE *f:files)std::fclose(f);
 files.clear();
}
std::string local_path(const char *path);
FILE *local_open(const char *path,const char *mode);
int local_close(FILE *file);
int local_mkdir(const char *path,int mode=0);
[[noreturn]] inline void fail_exit(int) {throw Failure(1,"Mesh2HRTF rejected the acoustic job; inspect the job's NC.out log");}
inline void frequency_progress(int current,int total) {
 check_now(); if(runtime->progress)runtime->progress(runtime->callbackContext,current,total);
}
} // namespace
