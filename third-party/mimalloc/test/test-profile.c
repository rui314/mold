/* ----------------------------------------------------------------------------
Copyright (c) 2026-2026, Microsoft Research, Daniel Schwartz-Narbonne, Daan Leijen
This is free software; you can redistribute it and/or modify it under the
terms of the MIT license. A copy of the license can be found in the file
"LICENSE" at the root of this distribution.
-----------------------------------------------------------------------------*/

// Tests for the mimalloc heap profiler (src/profile.c).

#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <stddef.h>

#include "mimalloc.h"
#include "mimalloc-profile.h"
#include "mimalloc/internal.h"
#include "testhelper.h"

// ---------------------------------------------------------------------------
// Shared callback state (not thread safe!)
// ---------------------------------------------------------------------------

typedef struct {
  mi_profiler_t profiler;
  uint64_t  alloc_count;
  uint64_t  free_count;
  size_t    last_size;
  uint64_t  last_upscaled;
  void*     last_ptr;
} my_profiler_t;

static inline my_profiler_t* downcast( mi_profiler_t* prof ) { 
  return (my_profiler_t*)prof; 
} 

// We store ptr in user_data so on_free can verify the round-trip.

#define TEST_THRESHOLD (16 * 1024)

static size_t mi_cdecl on_alloc(mi_profiler_t* profiler, mi_profiler_sample_data_t* data, void* ptr, size_t requested_size, size_t threshold, uint64_t bytes_since_last_sample, const mi_heap_t* heap) {
  MI_UNUSED(threshold); MI_UNUSED(heap); MI_UNUSED(requested_size);
  my_profiler_t* prof = downcast(profiler);
  assert(bytes_since_last_sample >= requested_size);  
  prof->alloc_count++;
  prof->last_ptr      = ptr;
  prof->last_size     = requested_size;
  prof->last_upscaled = bytes_since_last_sample;   
  // store ptr to verify round-trip 
  assert(data->user_data_size >= sizeof(void*));
  assert(data->user_data_size >= prof->profiler.sample_data_size);
  data->user_data[0] = ptr; 
  return TEST_THRESHOLD;
}

static void mi_cdecl on_free(mi_profiler_t* profiler, mi_profiler_sample_data_t* data, void* ptr, const mi_heap_t* heap) {
  MI_UNUSED(heap); MI_UNUSED_RELEASE(data); MI_UNUSED_RELEASE(ptr);
  my_profiler_t* prof = downcast(profiler);
  prof->free_count++;
  // verify the user_data round-trip
  assert(data->user_data[0] == ptr);  
}

static my_profiler_t my_profiler = {
  { // profiler_t
    NULL,            // reserved
    sizeof(void*),   // needed sample data size
    0,               // initial sample rate (default)        
    &on_alloc,       
    &on_free,
    NULL
  },
  0, 0, 0, 0, NULL
};




// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Force at least one sample by allocating well over the threshold.
static void allocate_past_threshold(void) {
  size_t total = 0;
  while (total < TEST_THRESHOLD * 10) {
    void* p = mi_malloc(4096);
    mi_free(p);
    total += 4096;
    // mi_collect()
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

bool test_profiler_samples(void) {
  CHECK_BODY("profiler: on_alloc called after threshold") {
    uint64_t before = my_profiler.alloc_count;
    allocate_past_threshold();
    result = (my_profiler.alloc_count > before);
  }
  return true;
}

#define MAXLOOP 100000

bool test_profiler_record_fields(void) {
  CHECK_BODY("profiler: record ptr and size are non-zero") {
    uint64_t before = my_profiler.alloc_count;
    int count;
    for (count = 0; my_profiler.alloc_count == before && count < MAXLOOP; count++) {
      void* p = mi_malloc(1024);
      mi_free(p);
    }
    assert(count!=MAXLOOP);    
    result = (my_profiler.last_ptr != NULL && my_profiler.last_size > 0 && my_profiler.last_upscaled > 0 && count!=MAXLOOP);
  }
  return true;
}

bool test_profiler_on_free_called(void) {
  CHECK_BODY("profiler: on_free called for sampled allocation") {
    uint64_t alloc_before = my_profiler.alloc_count;
    uint64_t free_before  = my_profiler.free_count;

    // Keep the pointer live until we confirm a sample was taken, then free it.
    void* sampled = NULL;
    int count;
    for (count = 0; my_profiler.alloc_count == alloc_before && count < MAXLOOP; count++) {
      if (sampled) { mi_free(sampled); }
      sampled = mi_malloc(1024);
    }
    // At this point my_profiler.last_ptr is the sampled pointer.
    // Free it and check on_free fires.
    void* expected = my_profiler.last_ptr;
    mi_free(expected);
    sampled = NULL;
    assert(count!=MAXLOOP);
    result = (my_profiler.free_count > free_before && count!=MAXLOOP);
  }
  return true;
}

bool test_profiler_upscaled_at_least_size(void) {
  CHECK_BODY("profiler: upscaled_size >= size") {
    uint64_t before = my_profiler.alloc_count;
    int count;
    for (count = 0; my_profiler.alloc_count == before && count < MAXLOOP; count++) {
      void* p = mi_malloc(256);
      mi_free(p);
    }
    assert(count!=MAXLOOP);
    result = (my_profiler.last_upscaled >= my_profiler.last_size && count!=MAXLOOP);
  }
  return true;
}

bool test_profiler_free_count_le_alloc_count(void) {
  CHECK_BODY("profiler: on_free never called more times than on_alloc") {
    // Free can only fire for sampled allocations, so free_count <= alloc_count
    // must hold at all times.
    allocate_past_threshold();
    result = (my_profiler.free_count <= my_profiler.alloc_count);
  }
  return true;
}


// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

int main(void) {
  mi_profile(&my_profiler.profiler);
  mi_profiler_start(&my_profiler.profiler);

  test_profiler_upscaled_at_least_size();
  test_profiler_samples();
  test_profiler_record_fields();
  test_profiler_on_free_called();
  test_profiler_free_count_le_alloc_count();

  mi_profiler_stop(&my_profiler.profiler);

  return print_test_summary();
}
