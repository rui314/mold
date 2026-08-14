#include "lib.h"

#ifdef _WIN32
# include <windows.h>
#endif

namespace mold {

static constexpr u32 BLOCK_SIZE = 64 * 1024;
static constexpr u32 BLOCK_ALIGNMENT = 64;
static constexpr u32 MAX_LOCAL_ALLOC = BLOCK_SIZE / 4;
static constexpr u32 NUM_ARENA_SLOTS = 10;

static std::atomic<u32> next_arena_slot;
static thread_local u32 positions[NUM_ARENA_SLOTS];
static thread_local u32 ends[NUM_ARENA_SLOTS];

[[noreturn]]
static void memory_error(std::string_view action, u64 size) {
  std::cerr << "mold: cannot " << action << ' ' << size
            << " bytes of virtual memory\n";
  std::exit(1);
}

ArenaResource::ArenaResource() : arena_slot(next_arena_slot++) {
  assert(arena_slot < NUM_ARENA_SLOTS);

#ifdef _WIN32
  data = (u8 *)VirtualAlloc(nullptr, (SIZE_T)SIZE, MEM_RESERVE, PAGE_NOACCESS);
  if (!data)
    memory_error("reserve", SIZE);
#else
  int flags = MAP_ANONYMOUS | MAP_PRIVATE;
# ifdef MAP_NORESERVE
  // The arena is much larger than most links need. Do not reserve swap for
  // pages that may never be touched.
  flags |= MAP_NORESERVE;
# endif
  data = (u8 *)mmap(nullptr, SIZE, PROT_READ | PROT_WRITE, flags, -1, 0);
  if (data == MAP_FAILED)
    memory_error("reserve", SIZE);
#endif

#ifdef MADV_HUGEPAGE
  // Large links fill the beginning of the arena densely. Transparent huge
  // pages reduce address-translation overhead without populating unused pages.
  madvise(data, SIZE, MADV_HUGEPAGE);
#endif
}

ArenaResource::~ArenaResource() {
#ifdef _WIN32
  VirtualFree(data, 0, MEM_RELEASE);
#else
  munmap(data, SIZE);
#endif
}

void *ArenaResource::allocate(u64 size, u64 alignment) {
  assert(std::has_single_bit(alignment));

  // ArenaPtr and base-relative indices both encode offsets in four-byte units.
  alignment = std::max<u64>(alignment, 4);

  // Standard containers make many small allocations. Reserve them from a
  // thread-local block to avoid contending on the global bump pointer.
  if (size <= MAX_LOCAL_ALLOC && alignment <= BLOCK_ALIGNMENT) {
    // Offsets are from the beginning of the arena in four-byte units.
    u32 &pos = positions[arena_slot];
    u32 &end = ends[arena_slot];
    u32 size4 = align_to(size, 4) / 4;

    if (pos) {
      u32 begin = align_to(pos, alignment / 4);
      if (begin <= end && size4 <= end - begin) {
        pos = begin + size4;
        return data + (u64)begin * 4;
      }
    }

    u8 *begin = (u8 *)allocate_global(BLOCK_SIZE, BLOCK_ALIGNMENT);
    u32 begin4 = (begin - data) / 4;
    pos = begin4 + size4;
    end = begin4 + BLOCK_SIZE / 4;
    return begin;
  }

  return allocate_global(size, alignment);
}

void *ArenaResource::allocate_global(u64 size, u64 alignment) {
  u64 old = offset.load(std::memory_order_relaxed);

  for (;;) {
    u64 padding = -old & (alignment - 1);
    if (old > SIZE || padding > SIZE - old || size > SIZE - old - padding) {
      std::cerr << "mold: cannot allocate more than " << SIZE
                << " bytes for linker data structures on this host\n";
      std::exit(1);
    }

    u64 begin = old + padding;
    u64 end = begin + size;
    if (offset.compare_exchange_weak(old, end, std::memory_order_relaxed)) {
#ifdef _WIN32
      // VirtualAlloc reserves and commits address space separately.
      if (size &&
          !VirtualAlloc(data + begin, (SIZE_T)size, MEM_COMMIT, PAGE_READWRITE))
        memory_error("commit", size);
#endif
      return data + begin;
    }
  }
}

} // namespace mold
