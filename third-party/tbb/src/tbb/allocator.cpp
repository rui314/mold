/*
    Copyright (c) 2005-2021 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#include "oneapi/tbb/version.h"

#include "oneapi/tbb/detail/_exception.h"
#include "oneapi/tbb/detail/_assert.h"
#include "oneapi/tbb/detail/_utils.h"

#include "dynamic_link.h"
#include "misc.h"

#include <cstdlib>

#if _WIN32 || _WIN64
#include <Windows.h>
#else
#include <dlfcn.h>
#endif /* _WIN32||_WIN64 */

#if __TBB_WEAK_SYMBOLS_PRESENT

#pragma weak scalable_malloc
#pragma weak scalable_free
#pragma weak scalable_aligned_malloc
#pragma weak scalable_aligned_free

extern "C" {
    void* scalable_malloc(std::size_t);
    void  scalable_free(void*);
    void* scalable_aligned_malloc(std::size_t, std::size_t);
    void  scalable_aligned_free(void*);
}

#endif /* __TBB_WEAK_SYMBOLS_PRESENT */

namespace tbb {
namespace detail {
namespace r1 {

//! Initialization routine used for first indirect call via allocate_handler.
static void* initialize_allocate_handler(std::size_t size);

//! Handler for memory allocation
static void* (*allocate_handler)(std::size_t size) = &initialize_allocate_handler;

//! Handler for memory deallocation
static void  (*deallocate_handler)(void* pointer) = nullptr;

//! Initialization routine used for first indirect call via cache_aligned_allocate_handler.
static void* initialize_cache_aligned_allocate_handler(std::size_t n, std::size_t alignment);

//! Allocates memory using standard malloc. It is used when scalable_allocator is not available
static void* std_cache_aligned_allocate(std::size_t n, std::size_t alignment);

//! Allocates memory using standard free. It is used when scalable_allocator is not available
static void  std_cache_aligned_deallocate(void* p);

//! Handler for padded memory allocation
static void* (*cache_aligned_allocate_handler)(std::size_t n, std::size_t alignment) = &initialize_cache_aligned_allocate_handler;

//! Handler for padded memory deallocation
static void (*cache_aligned_deallocate_handler)(void* p) = nullptr;

//! Table describing how to link the handlers.
static const dynamic_link_descriptor MallocLinkTable[] = {
    DLD(scalable_malloc, allocate_handler),
    DLD(scalable_free, deallocate_handler),
    DLD(scalable_aligned_malloc, cache_aligned_allocate_handler),
    DLD(scalable_aligned_free, cache_aligned_deallocate_handler),
};


#if TBB_USE_DEBUG
#define DEBUG_SUFFIX "_debug"
#else
#define DEBUG_SUFFIX
#endif /* TBB_USE_DEBUG */

// MALLOCLIB_NAME is the name of the oneTBB memory allocator library.
#if _WIN32||_WIN64
#define MALLOCLIB_NAME "tbbmalloc" DEBUG_SUFFIX ".dll"
#elif __APPLE__
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX ".dylib"
#elif __FreeBSD__ || __NetBSD__ || __OpenBSD__ || __sun || _AIX || __ANDROID__
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX ".so"
#elif __linux__  // Note that order of these #elif's is important!
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX ".so.2"
#else
#error Unknown OS
#endif

//! Initialize the allocation/free handler pointers.
/** Caller is responsible for ensuring this routine is called exactly once.
    The routine attempts to dynamically link with the TBB memory allocator.
    If that allocator is not found, it links to malloc and free. */
void initialize_handler_pointers() {
    __TBB_ASSERT(allocate_handler == &initialize_allocate_handler, NULL);
    bool success = dynamic_link(MALLOCLIB_NAME, MallocLinkTable, 4);
    if(!success) {
        // If unsuccessful, set the handlers to the default routines.
        // This must be done now, and not before FillDynamicLinks runs, because if other
        // threads call the handlers, we want them to go through the DoOneTimeInitializations logic,
        // which forces them to wait.
        allocate_handler = &std::malloc;
        deallocate_handler = &std::free;
        cache_aligned_allocate_handler = &std_cache_aligned_allocate;
        cache_aligned_deallocate_handler = &std_cache_aligned_deallocate;
    }

    PrintExtraVersionInfo( "ALLOCATOR", success?"scalable_malloc":"malloc" );
}

static std::once_flag initialization_state;
void initialize_cache_aligned_allocator() {
    std::call_once(initialization_state, &initialize_handler_pointers);
}

//! Executed on very first call through allocate_handler
static void* initialize_allocate_handler(std::size_t size) {
    initialize_cache_aligned_allocator();
    __TBB_ASSERT(allocate_handler != &initialize_allocate_handler, NULL);
    return (*allocate_handler)(size);
}

//! Executed on very first call through cache_aligned_allocate_handler
static void* initialize_cache_aligned_allocate_handler(std::size_t bytes, std::size_t alignment) {
    initialize_cache_aligned_allocator();
    __TBB_ASSERT(cache_aligned_allocate_handler != &initialize_cache_aligned_allocate_handler, NULL);
    return (*cache_aligned_allocate_handler)(bytes, alignment);
}

// TODO: use CPUID to find actual line size, though consider backward compatibility
// nfs - no false sharing
static constexpr std::size_t nfs_size = 128;

std::size_t __TBB_EXPORTED_FUNC cache_line_size() {
    return nfs_size;
}

void* __TBB_EXPORTED_FUNC cache_aligned_allocate(std::size_t size) {
    const std::size_t cache_line_size = nfs_size;
    __TBB_ASSERT(is_power_of_two(cache_line_size), "must be power of two");

    // Check for overflow
    if (size + cache_line_size < size) {
        throw_exception(exception_id::bad_alloc);
    }
    // scalable_aligned_malloc considers zero size request an error, and returns NULL
    if (size == 0) size = 1;

    void* result = cache_aligned_allocate_handler(size, cache_line_size);
    if (!result) {
        throw_exception(exception_id::bad_alloc);
    }
    __TBB_ASSERT(is_aligned(result, cache_line_size), "The returned address isn't aligned");
    return result;
}

void __TBB_EXPORTED_FUNC cache_aligned_deallocate(void* p) {
    __TBB_ASSERT(cache_aligned_deallocate_handler, "Initialization has not been yet.");
    (*cache_aligned_deallocate_handler)(p);
}

static void* std_cache_aligned_allocate(std::size_t bytes, std::size_t alignment) {
    // TODO: make it common with cache_aligned_resource
    std::size_t space = alignment + bytes;
    std::uintptr_t base = reinterpret_cast<std::uintptr_t>(std::malloc(space));
    if (!base) {
        return nullptr;
    }
    std::uintptr_t result = (base + nfs_size) & ~(nfs_size - 1);
    // Round up to the next cache line (align the base address)
    __TBB_ASSERT((result - base) >= sizeof(std::uintptr_t), "Cannot store a base pointer to the header");
    __TBB_ASSERT(space - (result - base) >= bytes, "Not enough space for the storage");

    // Record where block actually starts.
    (reinterpret_cast<std::uintptr_t*>(result))[-1] = base;
    return reinterpret_cast<void*>(result);
}

static void std_cache_aligned_deallocate(void* p) {
    if (p) {
        __TBB_ASSERT(reinterpret_cast<std::uintptr_t>(p) >= 0x4096, "attempt to free block not obtained from cache_aligned_allocator");
        // Recover where block actually starts
        std::uintptr_t base = (reinterpret_cast<std::uintptr_t*>(p))[-1];
        __TBB_ASSERT(((base + nfs_size) & ~(nfs_size - 1)) == reinterpret_cast<std::uintptr_t>(p), "Incorrect alignment or not allocated by std_cache_aligned_deallocate?");
        std::free(reinterpret_cast<void*>(base));
    }
}

void* __TBB_EXPORTED_FUNC allocate_memory(std::size_t size) {
    void* result = (*allocate_handler)(size);
    if (!result) {
        throw_exception(exception_id::bad_alloc);
    }
    return result;
}

void __TBB_EXPORTED_FUNC deallocate_memory(void* p) {
    if (p) {
        __TBB_ASSERT(deallocate_handler, "Initialization has not been yet.");
        (*deallocate_handler)(p);
    }
}

bool __TBB_EXPORTED_FUNC is_tbbmalloc_used() {
    if (allocate_handler == &initialize_allocate_handler) {
        void* void_ptr = allocate_handler(1);
        deallocate_handler(void_ptr);
    }
    __TBB_ASSERT(allocate_handler != &initialize_allocate_handler && deallocate_handler != nullptr, NULL);
    // Cast to void avoids type mismatch errors on some compilers (e.g. __IBMCPP__)
    __TBB_ASSERT((reinterpret_cast<void*>(allocate_handler) == reinterpret_cast<void*>(&std::malloc)) == (reinterpret_cast<void*>(deallocate_handler) == reinterpret_cast<void*>(&std::free)),
                  "Both shim pointers must refer to routines from the same package (either TBB or CRT)");
    return reinterpret_cast<void*>(allocate_handler) == reinterpret_cast<void*>(&std::malloc);
}

} // namespace r1
} // namespace detail
} // namespace tbb
