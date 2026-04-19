#include <cstddef>
#include <cstdint>

#include <oneapi/tbb/parallel_invoke.h>

#include "blake3_impl.h"

static_assert(TBB_USE_EXCEPTIONS == 0,
              "This file should be compiled with C++ exceptions disabled.");

extern "C" void blake3_compress_subtree_wide_join_tbb(
    // shared params
    const uint32_t key[8], uint8_t flags, bool use_tbb,
    // left-hand side params
    const uint8_t *l_input, size_t l_input_len, uint64_t l_chunk_counter,
    uint8_t *l_cvs, size_t *l_n,
    // right-hand side params
    const uint8_t *r_input, size_t r_input_len, uint64_t r_chunk_counter,
    uint8_t *r_cvs, size_t *r_n) noexcept {
  if (!use_tbb) {
    *l_n = blake3_compress_subtree_wide(l_input, l_input_len, key,
                                        l_chunk_counter, flags, l_cvs, use_tbb);
    *r_n = blake3_compress_subtree_wide(r_input, r_input_len, key,
                                        r_chunk_counter, flags, r_cvs, use_tbb);
    return;
  }

  oneapi::tbb::parallel_invoke(
      [=]() {
        *l_n = blake3_compress_subtree_wide(
            l_input, l_input_len, key, l_chunk_counter, flags, l_cvs, use_tbb);
      },
      [=]() {
        *r_n = blake3_compress_subtree_wide(
            r_input, r_input_len, key, r_chunk_counter, flags, r_cvs, use_tbb);
      });
}
