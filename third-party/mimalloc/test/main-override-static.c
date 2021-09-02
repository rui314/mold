#include <stdlib.h>
#include <stdio.h>
#include <assert.h>
#include <string.h>
#include <stdint.h>

#include <mimalloc.h>
#include <mimalloc-override.h>  // redefines malloc etc.

static void double_free1();
static void double_free2();
static void corrupt_free();
static void block_overflow1();
static void invalid_free();
static void test_aslr(void);
static void test_process_info(void);
static void test_reserved(void);
static void negative_stat(void);

int main() {
  mi_version();
  mi_stats_reset();
  // detect double frees and heap corruption
  // double_free1();
  // double_free2();
  // corrupt_free();
  // block_overflow1();
  // test_aslr();
  // invalid_free();
  // test_reserved();
  // negative_stat();
  
  void* p1 = malloc(78);
  void* p2 = malloc(24);
  free(p1);
  p1 = mi_malloc(8);
  char* s = strdup("hello\n");
  free(p2);
  
  p2 = malloc(16);
  p1 = realloc(p1, 32);
  free(p1);
  free(p2);
  free(s);
  
  /* now test if override worked by allocating/freeing across the api's*/
  //p1 = mi_malloc(32);
  //free(p1);
  //p2 = malloc(32);
  //mi_free(p2);
  mi_collect(true);
  mi_stats_print(NULL);
  // test_process_info();
  return 0;
}

static void invalid_free() {
  free((void*)0xBADBEEF);
  realloc((void*)0xBADBEEF,10);
}

static void block_overflow1() {
  uint8_t* p = (uint8_t*)mi_malloc(17);
  p[18] = 0;
  free(p);
}

// The double free samples come ArcHeap [1] by Insu Yun (issue #161)
// [1]: https://arxiv.org/pdf/1903.00503.pdf

static void double_free1() {
  void* p[256];
  //uintptr_t buf[256];

  p[0] = mi_malloc(622616);
  p[1] = mi_malloc(655362);
  p[2] = mi_malloc(786432);
  mi_free(p[2]);
  // [VULN] Double free
  mi_free(p[2]);
  p[3] = mi_malloc(786456);
  // [BUG] Found overlap
  // p[3]=0x429b2ea2000 (size=917504), p[1]=0x429b2e42000 (size=786432)
  fprintf(stderr, "p3: %p-%p, p1: %p-%p, p2: %p\n", p[3], (uint8_t*)(p[3]) + 786456, p[1], (uint8_t*)(p[1]) + 655362, p[2]);
}

static void double_free2() {
  void* p[256];
  //uintptr_t buf[256];
  // [INFO] Command buffer: 0x327b2000
  // [INFO] Input size: 182
  p[0] = malloc(712352);
  p[1] = malloc(786432);
  free(p[0]);
  // [VULN] Double free
  free(p[0]);
  p[2] = malloc(786440);
  p[3] = malloc(917504);
  p[4] = malloc(786440);
  // [BUG] Found overlap
  // p[4]=0x433f1402000 (size=917504), p[1]=0x433f14c2000 (size=786432)
  fprintf(stderr, "p1: %p-%p, p2: %p-%p\n", p[4], (uint8_t*)(p[4]) + 917504, p[1], (uint8_t*)(p[1]) + 786432);
}


// Try to corrupt the heap through buffer overflow
#define N   256
#define SZ  64

static void corrupt_free() {
  void* p[N];
  // allocate
  for (int i = 0; i < N; i++) {
    p[i] = malloc(SZ);
  }
  // free some
  for (int i = 0; i < N; i += (N/10)) {
    free(p[i]);
    p[i] = NULL;
  }
  // try to corrupt the free list
  for (int i = 0; i < N; i++) {
    if (p[i] != NULL) {
      memset(p[i], 0, SZ+8);
    }
  }
  // allocate more.. trying to trigger an allocation from a corrupted entry
  // this may need many allocations to get there (if at all)
  for (int i = 0; i < 4096; i++) {
    malloc(SZ);
  }
}

static void test_aslr(void) {
  void* p[256];
  p[0] = malloc(378200);
  p[1] = malloc(1134626);
  printf("p1: %p, p2: %p\n", p[0], p[1]);
}

static void test_process_info(void) {
  size_t elapsed = 0;
  size_t user_msecs = 0;
  size_t system_msecs = 0;
  size_t current_rss = 0;
  size_t peak_rss = 0;
  size_t current_commit = 0;
  size_t peak_commit = 0;
  size_t page_faults = 0;  
  for (int i = 0; i < 100000; i++) {
    void* p = calloc(100,10);
    free(p);
  }
  mi_process_info(&elapsed, &user_msecs, &system_msecs, &current_rss, &peak_rss, &current_commit, &peak_commit, &page_faults);
  printf("\n\n*** process info: elapsed %3zd.%03zd s, user: %3zd.%03zd s, rss: %zd b, commit: %zd b\n\n", elapsed/1000, elapsed%1000, user_msecs/1000, user_msecs%1000, peak_rss, peak_commit);
}

static void test_reserved(void) {
#define KiB 1024ULL
#define MiB (KiB*KiB)
#define GiB (MiB*KiB)
  mi_reserve_os_memory(4*GiB, false, true);
  void* p1 = malloc(100);
  void* p2 = malloc(100000);
  void* p3 = malloc(2*GiB);
  void* p4 = malloc(1*GiB + 100000);
  free(p1);
  free(p2);
  free(p3);
  p3 = malloc(1*GiB);
  free(p4);
}



static void negative_stat(void) {
  int* p = mi_malloc(60000);
  mi_stats_print_out(NULL, NULL);
  *p = 100;
  mi_free(p);
  mi_stats_print_out(NULL, NULL);  
}