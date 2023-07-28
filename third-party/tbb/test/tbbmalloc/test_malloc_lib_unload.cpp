/*
    Copyright (c) 2005-2022 Intel Corporation

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


#define __TBB_NO_IMPLICIT_LINKAGE 1

#if _USRDLL
#include "common/utils_assert.h"

const char *globalCallMsg = "A TBB allocator function call is resolved into wrong implementation.";

#if _WIN32||_WIN64
// must be defined in DLL for linker to not drop the dependency on the DLL.
extern "C" {
    extern __declspec(dllexport) void *scalable_malloc(size_t);
    extern __declspec(dllexport) void scalable_free (void *);
    extern __declspec(dllexport) void safer_scalable_free (void *, void (*)(void*));
    extern __declspec(dllexport) void *scalable_realloc(void *, size_t);
    extern __declspec(dllexport) void *safer_scalable_realloc(void *, size_t, void *);
    extern __declspec(dllexport) void *scalable_calloc(size_t, size_t);
    extern __declspec(dllexport) int scalable_posix_memalign(void **, size_t, size_t);
    extern __declspec(dllexport) void *scalable_aligned_malloc(size_t, size_t);
    extern __declspec(dllexport) void *scalable_aligned_realloc(void *, size_t, size_t);
    extern __declspec(dllexport) void *safer_scalable_aligned_realloc(void *, size_t, size_t, void *);
    extern __declspec(dllexport) void scalable_aligned_free(void *);
    extern __declspec(dllexport) size_t scalable_msize(void *);
    extern __declspec(dllexport) size_t safer_scalable_msize (void *, size_t (*)(void*));
    extern __declspec(dllexport) int anchor();
}
#endif

extern "C" int anchor() {
    return 42;
}

// Those functions must not be called instead of presented in dynamic library.
extern "C" void *scalable_malloc(size_t)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void scalable_free (void *)
{
    ASSERT(0, globalCallMsg);
}
extern "C" void safer_scalable_free (void *, void (*)(void*))
{
    ASSERT(0, globalCallMsg);
}
extern "C" void *scalable_realloc(void *, size_t)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void *safer_scalable_realloc(void *, size_t, void *)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void *scalable_calloc(size_t, size_t)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" int scalable_posix_memalign(void **, size_t, size_t)
{
    ASSERT(0, globalCallMsg);
    return 0;
}
extern "C" void *scalable_aligned_malloc(size_t, size_t)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void *scalable_aligned_realloc(void *, size_t, size_t)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void *safer_scalable_aligned_realloc(void *, size_t, size_t, void *)
{
    ASSERT(0, globalCallMsg);
    return nullptr;
}
extern "C" void scalable_aligned_free(void *)
{
    ASSERT(0, globalCallMsg);
}
extern "C" size_t scalable_msize(void *)
{
    ASSERT(0, globalCallMsg);
    return 0;
}
extern "C" size_t safer_scalable_msize (void *, size_t (*)(void*))
{
    ASSERT(0, globalCallMsg);
    return 0;
}

int main() {}

#else  // _USRDLL

#include "common/config.h"
// harness_defs.h must be included before tbb_stddef.h to overcome exception-dependent
// system headers that come from tbb_stddef.h
#if __TBB_WIN8UI_SUPPORT || __TBB_MIC_OFFLOAD || (__GNUC__ && __GNUC__ < 10 && __TBB_USE_SANITIZERS) || __TBB_SOURCE_DIRECTLY_INCLUDED
// The test does not work if dynamic load is unavailable.
// For MIC offload, it fails because liboffload brings libiomp which observes and uses the fake scalable_* calls.
// For sanitizers, it fails because RUNPATH is lost: https://github.com/google/sanitizers/issues/1219
#else
#include "common/test.h"
#include "common/memory_usage.h"
#include "common/utils_dynamic_libs.h"
#include "common/utils_assert.h"
#include "common/utils_report.h"
#include <cstring> // memset

extern "C" {
#if _WIN32||_WIN64
    extern __declspec(dllimport)
#endif
    void *scalable_malloc(size_t);

#if _WIN32||_WIN64
    extern __declspec(dllimport)
#endif
    int anchor();
}

struct Run {
    void operator()( std::size_t /*id*/ ) const {

        void* (*malloc_ptr)(std::size_t);
        void (*free_ptr)(void*);

        void* (*aligned_malloc_ptr)(size_t size, size_t alignment);
        void  (*aligned_free_ptr)(void*);

        const char* actual_name;
        utils::LIBRARY_HANDLE lib = utils::OpenLibrary(actual_name = MALLOCLIB_NAME1);
        if (!lib) lib = utils::OpenLibrary(actual_name = MALLOCLIB_NAME2);
        if (!lib) {
            REPORT("Can't load " MALLOCLIB_NAME1 " or " MALLOCLIB_NAME2 "\n");
            exit(1);
        }
        utils::GetAddress(lib, "scalable_malloc", malloc_ptr);
        utils::GetAddress(lib, "scalable_free", free_ptr);
        utils::GetAddress(lib, "scalable_aligned_malloc", aligned_malloc_ptr);
        utils::GetAddress(lib, "scalable_aligned_free", aligned_free_ptr);

        for (size_t sz = 1024; sz <= 10*1024 ; sz*=10) {
            void *p1 = aligned_malloc_ptr(sz, 16);
            std::memset(p1, 0, sz);
            aligned_free_ptr(p1);
        }

        void *p = malloc_ptr(100);
        std::memset(p, 1, 100);
        free_ptr(p);

        utils::CloseLibrary(lib);
#if _WIN32 || _WIN64
        ASSERT(GetModuleHandle(actual_name),
               "allocator library must not be unloaded");
#else
        ASSERT(dlsym(RTLD_DEFAULT, "scalable_malloc"),
               "allocator library must not be unloaded");
#endif
    }
};

//! \brief \ref error_guessing
TEST_CASE("test unload lib") {
    CHECK(anchor() == 42);

    // warm-up run
    utils::NativeParallelFor( 1, Run() );

    // It seems Thread Sanitizer remembers some history information about destroyed threads,
    // so memory consumption cannot be stabilized
    std::ptrdiff_t memory_leak = 0;
    {
        /* 1st call to GetMemoryUsage() allocate some memory,
           but it seems memory consumption stabilized after this.
        */
        utils::GetMemoryUsage();
        std::size_t memory_in_use = utils::GetMemoryUsage();
        std::size_t memory_check = utils::GetMemoryUsage();
        REQUIRE_MESSAGE(memory_in_use == memory_check,
            "Memory consumption should not increase after 1st GetMemoryUsage() call");
    }

    {
        // expect that memory consumption stabilized after several runs
        for (;;) {
            std::size_t memory_in_use = utils::GetMemoryUsage();
            for (int j=0; j<10; j++)
                utils::NativeParallelFor( 1, Run() );
            memory_leak = utils::GetMemoryUsage() - memory_in_use;
            if (memory_leak == 0)
                return;
        }
    }
}

#endif /* Unsupported configurations */

#endif // _USRDLL
