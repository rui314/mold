#include "lib.h"

#ifdef _WIN32
# include <windows.h>
#endif

namespace mold {

[[noreturn]]
static void memory_error(std::string_view action, u64 size) {
  std::cerr << "mold: cannot " << action << ' ' << size
            << " bytes of virtual memory\n";
  std::exit(1);
}

ArenaResource::ArenaResource() {
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
