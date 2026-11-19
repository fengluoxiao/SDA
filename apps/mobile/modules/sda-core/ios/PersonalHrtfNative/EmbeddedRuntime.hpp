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
struct Allocation {uint64_t bytes; void (*dispose)(void *);};
struct Runtime {
 SdaHrtfSolver &owner;
 std::filesystem::path work;
 uint64_t used=0,peak=0,ticks=0;
 std::unordered_map<void*,Allocation> arrays;
 std::vector<void*> order;
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
 if(count>uint64_t(std::numeric_limits<int>::max()) || count>runtime->owner.budget/sizeof(T))
  throw Failure(3,"Native solver array exceeds memory budget");
 uint64_t bytes=count*sizeof(T);
 if(bytes>runtime->owner.budget-runtime->used) throw Failure(3,"Native solver reached memory budget");
 T *p=new T[count];
 runtime->used+=bytes;runtime->peak=std::max(runtime->peak,runtime->used);
 try {
  runtime->arrays.emplace(p,Allocation{bytes,[](void *q){delete[] static_cast<T*>(q);}});
  runtime->order.push_back(p);
 } catch(...) {
  runtime->arrays.erase(p);runtime->used-=bytes;delete[] p;throw;
 }
 return p;
}
template<class T> void release_array(T *p) noexcept {
 if(!p) return;
 auto found=runtime->arrays.find(p);
 if(found==runtime->arrays.end()) std::terminate();
 auto a=found->second;runtime->arrays.erase(found);runtime->used-=a.bytes;a.dispose(p);
}
inline void Runtime::clear() noexcept {
 // Reverse creation order also handles partially constructed native arrays.
 for(auto p=order.rbegin();p!=order.rend();++p) {
  auto found=arrays.find(*p);if(found==arrays.end())continue;
  auto a=found->second;void *value=found->first;arrays.erase(found);used-=a.bytes;a.dispose(value);
 }
 order.clear();
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
