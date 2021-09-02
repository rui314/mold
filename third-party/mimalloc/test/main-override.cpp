#include <stdlib.h>
#include <stdio.h>
#include <assert.h>
#include <string.h>
#include <stdint.h>

#include <mimalloc.h>
#include <new>
#include <vector>
#include <future>
#include <iostream>

#include <thread>
#include <mimalloc.h>
#include <assert.h>

#ifdef _WIN32
#include <mimalloc-new-delete.h>
#endif

#ifdef _WIN32
#include <Windows.h>
static void msleep(unsigned long msecs) { Sleep(msecs); }
#else
#include <unistd.h>
static void msleep(unsigned long msecs) { usleep(msecs * 1000UL); }
#endif

void heap_thread_free_large(); // issue #221
void heap_no_delete();         // issue #202
void heap_late_free();         // issue #204
void padding_shrink();         // issue #209
void various_tests();
void test_mt_shutdown();
void fail_aslr();              // issue #372

int main() {
  mi_stats_reset();  // ignore earlier allocations
  heap_thread_free_large();
  heap_no_delete();
  heap_late_free();
  padding_shrink();
  various_tests();
  //test_mt_shutdown();
  //fail_aslr();
  mi_stats_print(NULL);
  return 0;
}

static void* p = malloc(8);

void free_p() {
  free(p);
  return;
}

class Test {
private:
  int i;
public:
  Test(int x) { i = x; }
  ~Test() { }
};


void various_tests() {
  atexit(free_p);
  void* p1 = malloc(78);
  void* p2 = mi_malloc_aligned(16, 24);
  free(p1);
  p1 = malloc(8);
  char* s = mi_strdup("hello\n");

  //char* s = _strdup("hello\n");
  //char* buf = NULL;
  //size_t len;
  //_dupenv_s(&buf,&len,"MIMALLOC_VERBOSE"); 
  //mi_free(buf);

  mi_free(p2);
  p2 = malloc(16);
  p1 = realloc(p1, 32);
  free(p1);
  free(p2);
  mi_free(s);
  Test* t = new Test(42);
  delete t;
  t = new (std::nothrow) Test(42);
  delete t;
}

class Static {
private:
  void* p;
public:
  Static() {
    p = malloc(64);
    return;
  }
  ~Static() {
    free(p);
    return;
  }
};

static Static s = Static();


bool test_stl_allocator1() {
  std::vector<int, mi_stl_allocator<int> > vec;
  vec.push_back(1);
  vec.pop_back();
  return vec.size() == 0;
}

struct some_struct { int i; int j; double z; };

bool test_stl_allocator2() {
  std::vector<some_struct, mi_stl_allocator<some_struct> > vec;
  vec.push_back(some_struct());
  vec.pop_back();
  return vec.size() == 0;
}



// Issue #202
void heap_no_delete_worker() {
  mi_heap_t* heap = mi_heap_new();
  void* q = mi_heap_malloc(heap, 1024);
  // mi_heap_delete(heap); // uncomment to prevent assertion
}

void heap_no_delete() {
  auto t1 = std::thread(heap_no_delete_worker);
  t1.join();
}


// Issue #204
volatile void* global_p;

void t1main() {
  mi_heap_t* heap = mi_heap_new();
  global_p = mi_heap_malloc(heap, 1024);
  mi_heap_delete(heap);
}

void heap_late_free() {
  auto t1 = std::thread(t1main);

  msleep(2000);
  assert(global_p);
  mi_free((void*)global_p);

  t1.join();
}

// issue  #209
static void* shared_p;
static void alloc0(/* void* arg */)
{
  shared_p = mi_malloc(8);
}

void padding_shrink(void)
{
  auto t1 = std::thread(alloc0);
  t1.join();
  mi_free(shared_p);
}


// Issue #221
void heap_thread_free_large_worker() {
  mi_free(shared_p);
}

void heap_thread_free_large() {
  for (int i = 0; i < 100; i++) {
    shared_p = mi_malloc_aligned(2*1024*1024 + 1, 8);
    auto t1 = std::thread(heap_thread_free_large_worker);
    t1.join();
  }
}



void test_mt_shutdown()
{
  const int threads = 5;
  std::vector< std::future< std::vector< char* > > > ts;

  auto fn = [&]()
  {
    std::vector< char* > ps;
    ps.reserve(1000);
    for (int i = 0; i < 1000; i++)
      ps.emplace_back(new char[1]);
    return ps;
  };

  for (int i = 0; i < threads; i++)
    ts.emplace_back(std::async(std::launch::async, fn));

  for (auto& f : ts)
    for (auto& p : f.get())
      delete[] p;

  std::cout << "done" << std::endl;
}

// issue #372
void fail_aslr() {
  size_t sz = (4ULL << 40); // 4TiB
  void* p = malloc(sz);
  printf("pointer p: %p: area up to %p\n", p, (uint8_t*)p + sz);
  *(int*)0x5FFFFFFF000 = 0;  // should segfault
}