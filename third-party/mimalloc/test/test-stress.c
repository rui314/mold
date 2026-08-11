/* ----------------------------------------------------------------------------
Copyright (c) 2018-2026 Microsoft Research, Daan Leijen
This is free software; you can redistribute it and/or modify it under the
terms of the MIT license.
-----------------------------------------------------------------------------*/

/* This is a stress test for the allocator, using multiple threads and
   transferring objects between threads. It tries to reflect real-world workloads:
   - allocation size is distributed linearly in powers of two
   - with some fraction extra large (and some very large)
   - the allocations are initialized and read again at free
   - pointers transfer between threads
   - threads are terminated and recreated with some objects surviving in between
   - uses deterministic "randomness", but execution can still depend on
     (random) thread scheduling. Do not use this test as a benchmark!
*/

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <assert.h>

#include <mimalloc.h>
#include <mimalloc-stats.h>

// #define MI_GUARDED         1
// #define USE_STD_MALLOC     1

// #define MI_USE_HEAPS        1
// #define ALLOW_LARGE         1
// #define TEST_STRESS_SUBPROCS   1
// #define TEST_LEAK              1

#define TEST_STRESS            1

#ifndef NTHREADS
#define NTHREADS               32
#endif

// > mimalloc-test-stress [THREADS] [SCALE] [ITER]
//
// argument defaults
#if defined(MI_TSAN)          // with thread-sanitizer reduce the threads to test within the azure pipeline limits
static int THREADS = NTHREADS/4;
static int SCALE   = 25;
static int ITER    = 400;
#elif defined(MI_UBSAN)       // with undefined behavious sanitizer reduce parameters to stay within the azure pipeline limits
static int THREADS = NTHREADS/4;
static int SCALE   = 25;
static int ITER    = 20;
#elif defined(MI_GUARDED)     // with debug guard pages reduce parameters to stay within the azure pipeline limits
static int THREADS = NTHREADS/4;
static int SCALE   = 25;
static int ITER    = 10;
#elif 0
static int THREADS = NTHREADS;
static int SCALE   = 25;
static int ITER    = 50;
#define ALLOW_LARGE true
#else
static int THREADS = NTHREADS;      // more repeatable if THREADS <= #processors
static int SCALE   = 50;            // scaling factor
static int ITER    = 50;            // N full iterations destructing and re-creating all threads
#endif

#ifndef ALLOW_LARGE
#define ALLOW_LARGE  false
#endif


static bool   allow_large_objects = ALLOW_LARGE;    // allow very large objects? (set to `true` if SCALE>100)

static size_t use_one_size = 0;               // use single object size of `N * sizeof(uintptr_t)`?

static bool   main_participates = false;       // main thread participates as a worker too

#ifdef USE_STD_MALLOC

#define custom_calloc(n,s)    calloc(n,s)
#define custom_realloc(p,s)   realloc(p,s)
#define custom_free(p)        free(p)

#else

#ifdef MI_USE_HEAPS
#if TEST_STRESS_SUBPROCS
#error "cannot test rolling heaps with multiple subprocesses (for now)"
#endif
static mi_heap_t* current_heap;
#define custom_calloc(n,s)    mi_heap_calloc(current_heap,n,s)
#define custom_realloc(p,s)   mi_heap_realloc(current_heap,p,s)
#define custom_free(p)        mi_free(p)
#else
#define custom_calloc(n,s)    mi_calloc(n,s)
#define custom_realloc(p,s)   mi_realloc(p,s)
#define custom_free(p)        mi_free(p)
#endif

#ifndef NDEBUG
#define xMI_HEAP_WALK             // walk the theap objects?
#endif

#endif

// transfer pointer between threads
#define TRANSFERS     (1000)
// static volatile void* transfer[TRANSFERS];


#if (UINTPTR_MAX != UINT32_MAX)
const uintptr_t cookie = 0xbf58476d1ce4e5b9UL;
#else
const uintptr_t cookie = 0x1ce4e5b9UL;
#endif

static void* atomic_exchange_ptr(volatile void** p, void* newval);

typedef uintptr_t* random_t;

static uintptr_t pick(random_t r) {
  uintptr_t x = *r;
#if (UINTPTR_MAX > UINT32_MAX)
  // by Sebastiano Vigna, see: <http://xoshiro.di.unimi.it/splitmix64.c>
  x ^= x >> 30;
  x *= 0xbf58476d1ce4e5b9UL;
  x ^= x >> 27;
  x *= 0x94d049bb133111ebUL;
  x ^= x >> 31;
#else
  // by Chris Wellons, see: <https://nullprogram.com/blog/2018/07/31/>
  x ^= x >> 16;
  x *= 0x7feb352dUL;
  x ^= x >> 15;
  x *= 0x846ca68bUL;
  x ^= x >> 16;
#endif
  *r = x;
  return x;
}

static bool chance(size_t perc, random_t r) {
  return (pick(r) % 100 <= perc);
}

static void* alloc_items(size_t items, random_t r) {
  if (chance(1, r)) {
    if (chance(1, r) && allow_large_objects) items *= 10000;       // 0.01% giant
    else if (chance(10, r) && allow_large_objects) items *= 1000;  // 0.1% huge
    else items *= 100;                                             // 1% large objects;
  }
  if (items>=32 && items<=40) items*=2;              // pthreads uses 320b allocations (this shows that more clearly in the stats)
  if (use_one_size > 0) items = (use_one_size / sizeof(uintptr_t));
  if (items==0) items = 1;
  uintptr_t* p = (uintptr_t*)custom_calloc(items,sizeof(uintptr_t));
  if (p != NULL) {
    for (uintptr_t i = 0; i < items; i++) {
      assert(p[i] == 0);
      p[i] = (items - i) ^ cookie;
    }
  }
  return p;
}

static void free_items(void* p) {
  if (p != NULL) {
    uintptr_t* q = (uintptr_t*)p;
    uintptr_t items = (q[0] ^ cookie);
    for (uintptr_t i = 0; i < items; i++) {
      if ((q[i] ^ cookie) != items - i) {
        fprintf(stderr, "memory corruption at block %p at %zu\n", p, i);
        abort();
      }
    }
  }
  custom_free(p);
}

#ifdef MI_HEAP_WALK
static bool visit_blocks(const mi_theap_t* theap, const mi_theap_area_t* area, void* block, size_t block_size, void* arg) {
  (void)(theap); (void)(area);
  size_t* total = (size_t*)arg;
  if (block != NULL) {
    *total += block_size;
  }
  return true;
}
#endif

static void stress(intptr_t tid, void* vtransfers) {
  #ifndef USE_STD_MALLOC
  // printf("test stress thread: subproc: %p, tid: %zi\n", mi_subproc_current()._mi_subproc_id, tid);
  #endif
  volatile void** transfers = (volatile void**)vtransfers;
  //bench_start_thread();
  uintptr_t r = ((tid + 1) * 43); // rand();
  const size_t max_item_shift = 5; // 128
  const size_t max_item_retained_shift = max_item_shift + 2;
  size_t allocs = 100 * ((size_t)SCALE) * (tid % 8 + 1); // some threads do more
  size_t retain = allocs / 2;
  void** data = NULL;
  size_t data_size = 0;
  size_t data_top = 0;
  void** retained = (void**)custom_calloc(retain,sizeof(void*));
  size_t retain_top = 0;

  while (allocs > 0 || retain > 0) {
    if (retain == 0 || (chance(50, &r) && allocs > 0)) {
      // 50%+ alloc
      allocs--;
      if (data_top >= data_size) {
        data_size += 100000;
        data = (void**)custom_realloc(data, data_size * sizeof(void*));
      }
      data[data_top++] = alloc_items(1ULL << (pick(&r) % max_item_shift), &r);
    }
    else {
      // 25% retain
      retained[retain_top++] = alloc_items( 1ULL << (pick(&r) % max_item_retained_shift), &r);
      retain--;
    }
    if (chance(66, &r) && data_top > 0) {
      // 66% free previous alloc
      size_t idx = pick(&r) % data_top;
      free_items(data[idx]);
      data[idx] = NULL;
    }
    if (chance(25, &r) && data_top > 0) {
      // 25% exchange a local pointer with the (shared) transfer buffer.
      size_t data_idx = pick(&r) % data_top;
      size_t transfer_idx = pick(&r) % TRANSFERS;
      void* p = data[data_idx];
      void* q = atomic_exchange_ptr(&transfers[transfer_idx], p);
      data[data_idx] = q;
    }
  }

  #ifdef MI_HEAP_WALK
  // walk the theap
  size_t total = 0;
  mi_theap_visit_blocks(mi_theap_get_default(), true, visit_blocks, &total);
  #endif

  // free everything that is left
  for (size_t i = 0; i < retain_top; i++) {
    free_items(retained[i]);
  }
  for (size_t i = 0; i < data_top; i++) {
    free_items(data[i]);
  }
  custom_free(retained);
  custom_free(data);
  //bench_end_thread();
}

static mi_subproc_id_t subproc_null = { NULL };

typedef void (thread_entry_fun_t)(intptr_t tid, void* arg);

static void run_os_threads(mi_subproc_id_t subproc, size_t nthreads, thread_entry_fun_t* fun, void* arg);

static void test_stress(mi_subproc_id_t subproc) {
  // printf("test stress: subproc: %p\n", subproc._mi_subproc_id);
  volatile void* transfers[TRANSFERS];
  memset((void**)transfers,0,sizeof(transfers));

  #ifdef MI_USE_HEAPS
  mi_heap_t* prev_heaps[MI_USE_HEAPS] = { NULL };
  #endif
  uintptr_t r = rand();
  for (int n = 0; n < ITER; n++) {

    #ifdef MI_USE_HEAPS
    // new heap for each iteration
    if (prev_heaps[MI_USE_HEAPS-1] != NULL) {
      mi_heap_delete(prev_heaps[MI_USE_HEAPS-1]);   // delete from N iterations ago
    }
    for(int i = MI_USE_HEAPS-1; i > 0; i--) {
      prev_heaps[i] = prev_heaps[i-1];
    }
    prev_heaps[0] = current_heap;
    current_heap = mi_heap_new();
    #endif

    run_os_threads(subproc, THREADS, &stress, (void**)transfers);

    #if !defined(NDEBUG) && !defined(USE_STD_MALLOC)
    // switch between arena and OS allocation for testing
    // mi_option_set_enabled(mi_option_disallow_arena_alloc, (n%2)==1);
    #endif
    #if defined(MI_HEAP_WALK) && defined(MI_USE_HEAPS)
    size_t total = 0;
    // mi_abandoned_visit_blocks(mi_subproc_main(), -1, true, visit_blocks, &total);
    mi_heap_visit_blocks(heap, true, visit_blocks, &total);
    #endif

    for (int i = 0; i < TRANSFERS; i++) {
      if (chance(50, &r) || n + 1 == ITER) { // free all on last run, otherwise free half of the transfers
        void* p = atomic_exchange_ptr(&transfers[i], NULL);
        free_items(p);
      }
    }

    #if !defined(NDEBUG) || defined(MI_TSAN)
    if ((n + 1) % 10 == 0) {
      printf("- iterations left: %3d\n", ITER - (n + 1));
      #ifndef USE_STD_MALLOC
      // mi_debug_show_arenas();
      #endif
      //mi_collect(true);
      //mi_debug_show_arenas();
    }
    #endif
  }

  #ifndef USE_STD_MALLOC
  mi_stats_print(NULL);
  #endif

  // clean up  (a bit too early to test the final free_items still works correctly)
  #ifdef MI_USE_HEAPS
  for (int i = 0; i < MI_USE_HEAPS; i++) {
    mi_heap_delete(prev_heaps[i]); prev_heaps[i] = NULL;
  }
  mi_heap_delete(current_heap); current_heap = NULL;
  #endif

  for (int i = 0; i < TRANSFERS; i++) {
    void* p = atomic_exchange_ptr(&transfers[i], NULL);
    if (p != NULL) {
      free_items(p);
    }
  }
}

#if TEST_STRESS_SUBPROCS && !defined(USE_STD_MALLOC)
static mi_subproc_id_t subprocs[NSUBPROCS];

static void test_stress_subproc( intptr_t i, void* arg ) {
  (void)arg;
  mi_subproc_id_t subproc = subprocs[i];
  mi_subproc_add_current_thread(subproc);
  test_stress(subproc);
}

static void test_stress_subprocs(void) {
  printf(" (for %d subprocesses)\n", NSUBPROCS);

  for(int i = 0; i < NSUBPROCS; i++) {
    subprocs[i] = mi_subproc_new();
  }
  run_os_threads(subproc_null, NSUBPROCS, &test_stress_subproc, NULL);
  for(int i = 0; i < NSUBPROCS; i++) {
    mi_subproc_destroy(subprocs[i]);
  }
}
#endif

#if TEST_LEAK
static void leak(intptr_t tid) {
  uintptr_t r = rand();
  void* p = alloc_items(1 /*pick(&r)%128*/, &r);
  if (chance(50, &r)) {
    intptr_t i = (pick(&r) % TRANSFERS);
    void* q = atomic_exchange_ptr(&transfer[i], p);
    free_items(q);
  }
}

static void test_leak(void) {
  for (int n = 0; n < ITER; n++) {
    run_os_threads(subproc_null, THREADS, &leak, NULL);
    mi_collect(false);
#ifndef NDEBUG
    if ((n + 1) % 10 == 0) { printf("- iterations left: %3d\n", ITER - (n + 1)); }
#endif
  }
}
#endif

#if defined(USE_STD_MALLOC) && defined(MI_LINK_VERSION)
#ifdef __cplusplus
extern "C"
#endif
int mi_version(void);
#endif

int main(int argc, char** argv) {
  #ifdef MI_LINK_VERSION
    mi_version();
  #endif  
  #if !defined(NDEBUG) && !defined(USE_STD_MALLOC)
    mi_option_set(mi_option_arena_reserve, (long)(mi_arena_min_size()/1024) /* in KiB ! */);
    mi_option_set(mi_option_purge_delay,1);
  #endif
  #if defined(NDEBUG) && !defined(USE_STD_MALLOC)
    // mi_option_set(mi_option_purge_delay,-1);
    mi_option_set(mi_option_page_reclaim_on_free, 0);
  #endif

  // > mimalloc-test-stress [THREADS] [SCALE] [ITER]
  if (argc >= 2) {
    char* end;
    long n = strtol(argv[1], &end, 10);
    if (n > 0) THREADS = n;
  }
  if (argc >= 3) {
    char* end;
    long n = (strtol(argv[2], &end, 10));
    if (n > 0) SCALE = n;
  }
  if (argc >= 4) {
    char* end;
    long n = (strtol(argv[3], &end, 10));
    if (n > 0) ITER = n;
  }
  if (SCALE > 100) {
    allow_large_objects = true;
  }
  printf("Using %d threads with a %d%% load-per-thread and %d iterations%s", THREADS, SCALE, ITER, (allow_large_objects ? " (allow large objects)" : ""));
  #if MI_USE_HEAPS
  printf(" (using %d rolling heaps)", MI_USE_HEAPS);
  #endif
  printf("\n"); fflush(stdout);

  #if !defined(NDEBUG) && !defined(USE_STD_MALLOC)
  mi_stats_reset();
  #endif

  //mi_reserve_os_memory(1024*1024*1024ULL, false, true);
  //int res = mi_reserve_huge_os_pages(4,1);
  //printf("(reserve huge: %i\n)", res);

  //bench_start_program();

  // Run ITER full iterations where half the objects in the transfer buffer survive to the next round.
  srand(0x7feb352d);
  // mi_stats_reset();
#if TEST_STRESS_SUBPROCS && !defined(USE_STD_MALLOC)
    test_stress_subprocs();
#elif TEST_STRESS
    test_stress(subproc_null);
#elif TEST_LEAK
    test_leak();
#endif

#ifndef USE_STD_MALLOC
  #ifndef NDEBUG
  mi_collect(true);
  mi_debug_show_arenas();
  //mi_collect(true);
  //char* json = mi_stats_get_json(0, NULL);
  //if (json != NULL) {
  //  fputs(json,stderr);
  //  mi_free(json);
  //}
  #endif
  mi_collect(true);
  mi_stats_print(NULL);
#endif
  //bench_end_program();
  return 0;
}


typedef struct callback_s {
  thread_entry_fun_t* fun;
  intptr_t tid;
  void*    arg;
  mi_subproc_id_t subproc;
} callback_t;

static void* thread_entry(void* param) {
  callback_t* cb = (callback_t*)param;
  #ifndef USE_STD_MALLOC
  if (cb->subproc._mi_subproc_id != NULL) {
    mi_subproc_add_current_thread(cb->subproc);
  }
  #endif
  cb->fun(cb->tid,cb->arg);
  return NULL;
}


#ifdef _WIN32

#include <windows.h>

static DWORD WINAPI win_thread_entry(LPVOID param) {
  thread_entry(param);
  return 0;
}

static void run_os_threads(mi_subproc_id_t subproc, size_t nthreads, thread_entry_fun_t* fun, void* arg) {
  DWORD* tids = (DWORD*)custom_calloc(nthreads,sizeof(DWORD));
  HANDLE* thandles = (HANDLE*)custom_calloc(nthreads,sizeof(HANDLE));
  callback_t* callbacks = (callback_t*)custom_calloc(nthreads,sizeof(callback_t));
  thandles[0] = GetCurrentThread(); // avoid lint warning
  const size_t start = (main_participates ? 1 : 0);
  for (size_t i = start; i < nthreads; i++) {
    callbacks[i].fun = fun;
    callbacks[i].tid = i;
    callbacks[i].arg = arg;
    callbacks[i].subproc = subproc;
    thandles[i] = CreateThread(0, 8*1024L, &win_thread_entry, (void*)&callbacks[i], 0, &tids[i]);
  }
  if (main_participates) {
    fun(0,arg); // run the main thread as well
  }
  for (size_t i = start; i < nthreads; i++) {
    WaitForSingleObject(thandles[i], INFINITE);
  }
  for (size_t i = start; i < nthreads; i++) {
    CloseHandle(thandles[i]);
  }
  custom_free(callbacks);
  custom_free(tids);
  custom_free(thandles);
}

static void* atomic_exchange_ptr(volatile void** p, void* newval) {
#if (INTPTR_MAX == INT32_MAX)
  return (void*)InterlockedExchange((volatile LONG*)p, (LONG)newval);
#else
  return (void*)InterlockedExchange64((volatile LONG64*)p, (LONG64)newval);
#endif
}
#else

#include <pthread.h>

static void run_os_threads(mi_subproc_id_t subproc, size_t nthreads, thread_entry_fun_t* fun, void* arg) {
  pthread_t* threads = (pthread_t*)custom_calloc(nthreads,sizeof(pthread_t));
  callback_t* callbacks = (callback_t*)custom_calloc(nthreads,sizeof(callback_t));
  const size_t start = (main_participates ? 1 : 0);
  //pthread_setconcurrency(nthreads);
  for (size_t i = start; i < nthreads; i++) {
    callbacks[i].fun = fun;
    callbacks[i].tid = i;
    callbacks[i].arg = arg;
    callbacks[i].subproc = subproc;
    pthread_create(&threads[i], NULL, &thread_entry, (void*)&callbacks[i]);
  }
  if (main_participates) {
    fun(0,arg); // run the main thread as well
  }
  for (size_t i = start; i < nthreads; i++) {
    pthread_join(threads[i], NULL);
  }
  custom_free(callbacks);
  custom_free(threads);
}

#ifdef __cplusplus
#include <atomic>
static void* atomic_exchange_ptr(volatile void** p, void* newval) {
  return std::atomic_exchange((volatile std::atomic<void*>*)p, newval);
}
#else
#include <stdatomic.h>
static void* atomic_exchange_ptr(volatile void** p, void* newval) {
  return atomic_exchange((volatile _Atomic(void*)*)p, newval);
}
#endif

#endif
