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

#include <stdlib.h>
#include "common/config.h"
#include "common/utils_assert.h"
#include "tbb/scalable_allocator.h"

#if _WIN32||_WIN64
extern "C" {
    extern __declspec(dllexport) void callDll();
}
#endif

extern "C" void callDll()
{
    static const int NUM = 20;
    void *ptrs[NUM];

    for (int i=0; i<NUM; i++) {
        ptrs[i] = scalable_malloc(i*1024);
        ASSERT(ptrs[i], nullptr);
    }
    for (int i=0; i<NUM; i++)
        scalable_free(ptrs[i]);
}

int main() {}


#else // _USRDLL
#include "common/config.h"
// FIXME: fix the test to support Windows* 8 Store Apps mode.
// For sanitizers, it fails because RUNPATH is lost: https://github.com/google/sanitizers/issues/1219
#if !__TBB_WIN8UI_SUPPORT && !(__GNUC__ && __GNUC__ < 10 && __TBB_USE_SANITIZERS) && __TBB_DYNAMIC_LOAD_ENABLED

#define __TBB_NO_IMPLICIT_LINKAGE 1
#include "common/test.h"
#include "common/utils.h"
#include "common/utils_dynamic_libs.h"
#include "common/utils_report.h"
#include "common/memory_usage.h"
#include "common/spin_barrier.h"


class UseDll {
    utils::FunctionAddress run;
public:
    UseDll(utils::FunctionAddress runPtr) : run(runPtr) { }
    void operator()( std::size_t /*id*/ ) const {
        (*run)();
    }
};

void LoadThreadsUnload()
{
    utils::LIBRARY_HANDLE lib =
        utils::OpenLibrary(TEST_LIBRARY_NAME("_test_malloc_used_by_lib"));
    ASSERT(lib, "Can't load " TEST_LIBRARY_NAME("_test_malloc_used_by_lib"));
    utils::NativeParallelFor(std::size_t(4), UseDll(utils::GetAddress(lib, "callDll")));
    utils::CloseLibrary(lib);
}

struct UnloadCallback {
    utils::LIBRARY_HANDLE lib;

    void operator() () const {
        utils::CloseLibrary(lib);
    }
};

struct RunWithLoad {
    static utils::SpinBarrier startBarr, endBarr;
    static UnloadCallback unloadCallback;
    static utils::FunctionAddress runPtr;

    void operator()(std::size_t id) const {
        if (!id) {
            utils::LIBRARY_HANDLE lib =
                utils::OpenLibrary(TEST_LIBRARY_NAME("_test_malloc_used_by_lib"));
            ASSERT(lib, "Can't load " TEST_LIBRARY_NAME("_test_malloc_used_by_lib"));
            runPtr = utils::GetAddress(lib, "callDll");
            unloadCallback.lib = lib;
        }
        startBarr.wait();
        (*runPtr)();
        endBarr.wait(unloadCallback);
    }
};

utils::SpinBarrier RunWithLoad::startBarr{}, RunWithLoad::endBarr{};
UnloadCallback RunWithLoad::unloadCallback;
utils::FunctionAddress RunWithLoad::runPtr;

void ThreadsLoadUnload() {
    constexpr std::size_t threads = 4;

    RunWithLoad::startBarr.initialize(threads);
    RunWithLoad::endBarr.initialize(threads);
    RunWithLoad body{};
    utils::NativeParallelFor(threads, body);
}

//! \brief \ref error_guessing
TEST_CASE("use test as lib") {
    const int ITERS = 20;
    int i;
    std::ptrdiff_t memory_leak = 0;

    utils::GetMemoryUsage();

    for (int run = 0; run < 2; run++) {
        // expect that memory consumption stabilized after several runs
        for (i = 0; i < ITERS; i++) {
            std::size_t memory_in_use = utils::GetMemoryUsage();
            if (run) {
                LoadThreadsUnload();
            } else {
                ThreadsLoadUnload();
            }
            memory_leak = utils::GetMemoryUsage() - memory_in_use;
            if (memory_leak == 0)  // possibly too strong?
                break;
        }
        if(i==ITERS) {
            // not stabilized, could be leak
            REPORT( "Error: memory leak of up to %ld bytes\n", static_cast<long>(memory_leak));
            WARN(false);
        }
    }
}
#endif /* Unsupported configurations */
#endif // _USRDLL
