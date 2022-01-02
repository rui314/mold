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

#define __TBB_NO_IMPLICIT_LINKAGE 1

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/memory_usage.h"
#include "oneapi/tbb/detail/_utils.h"
#include "tbb/scalable_allocator.h"
#include "thread"
#include <stdio.h>

class minimalAllocFree {
public:
    void operator()(int size) const {
        tbb::scalable_allocator<char> a;
        char* str = a.allocate( size );
        a.deallocate( str, size );
    }
};


template<typename Body, typename Arg>
void RunThread(const Body& body, const Arg& arg) {
    std::thread job(body, arg);
    job.join();
}

/*--------------------------------------------------------------------*/
// The regression test against bug #1518 where thread bootstrap allocations "leaked"

bool TestBootstrapLeak() {
    /* In the bug 1518, each thread leaked ~384 bytes.
       Initially, scalable allocator maps 1MB. Thus it is necessary to take out most of this space.
       1MB is chunked into 16K blocks; of those, one block is for thread bootstrap, and one more
       should be reserved for the test body. 62 blocks left, each can serve 15 objects of 1024 bytes.
    */
    const int alloc_size = 1024;
    const int take_out_count = 15*62;

    tbb::scalable_allocator<char> a;
    char* array[take_out_count];
    for( int i=0; i<take_out_count; ++i )
        array[i] = a.allocate( alloc_size );

    RunThread( minimalAllocFree(), alloc_size ); // for threading library to take some memory
    size_t memory_in_use = utils::GetMemoryUsage();
    // Wait for memory usage data to "stabilize". The test number (1000) has nothing underneath.
    for( int i=0; i<1000; i++) {
        if( utils::GetMemoryUsage()!=memory_in_use ) {
            memory_in_use = utils::GetMemoryUsage();
            i = -1;
        }
    }

    ptrdiff_t memory_leak = 0;
    // Note that 16K bootstrap memory block is enough to serve 42 threads.
    const int num_thread_runs = 200;
    for (;;) {
        memory_in_use = utils::GetMemoryUsage();
        for( int i=0; i<num_thread_runs; ++i )
            RunThread( minimalAllocFree(), alloc_size );

        memory_leak = utils::GetMemoryUsage() - memory_in_use;
        if (!memory_leak)
            break;
    }
    if( memory_leak>0 ) { // possibly too strong?
        REPORT( "Error: memory leak of up to %ld bytes\n", static_cast<long>(memory_leak));
    }

    for( int i=0; i<take_out_count; ++i )
        a.deallocate( array[i], alloc_size );

    return memory_leak<=0;
}

/*--------------------------------------------------------------------*/
// The regression test against a bug with incompatible semantics of msize and realloc

bool TestReallocMsize(size_t startSz) {
    bool passed = true;

    char *buf = (char*)scalable_malloc(startSz);
    REQUIRE_MESSAGE(buf, "");
    size_t realSz = scalable_msize(buf);
    REQUIRE_MESSAGE(realSz>=startSz, "scalable_msize must be not less then allocated size");
    memset(buf, 'a', realSz-1);
    buf[realSz-1] = 0;
    char *buf1 = (char*)scalable_realloc(buf, 2*realSz);
    REQUIRE_MESSAGE(buf1, "");
    REQUIRE_MESSAGE((scalable_msize(buf1)>=2*realSz), "scalable_msize must be not less then allocated size");
    buf1[2*realSz-1] = 0;
    if ( strspn(buf1, "a") < realSz-1 ) {
        REPORT( "Error: data broken for %d Bytes object.\n", startSz);
        passed = false;
    }
    scalable_free(buf1);

    return passed;
}

// regression test against incorrect work of msize/realloc
// for aligned objects
void TestAlignedMsize()
{
    const int NUM = 4;
    char *p[NUM];
    size_t objSizes[NUM];
    size_t allocSz[] = {4, 8, 512, 2*1024, 4*1024, 8*1024, 16*1024, 0};
    size_t align[] = {8, 512, 2*1024, 4*1024, 8*1024, 16*1024, 0};

    for (int a=0; align[a]; a++)
        for (int s=0; allocSz[s]; s++) {
            for (int i=0; i<NUM; i++) {
                p[i] = (char*)scalable_aligned_malloc(allocSz[s], align[a]);
                CHECK_FAST(tbb::detail::is_aligned(p[i], align[a]));
            }

            for (int i=0; i<NUM; i++) {
                objSizes[i] = scalable_msize(p[i]);
                CHECK_FAST_MESSAGE(objSizes[i] >= allocSz[s], "allocated size must be not less than requested");
                memset(p[i], i, objSizes[i]);
            }
            for (int i=0; i<NUM; i++) {
                for (unsigned j=0; j<objSizes[i]; j++)
                    CHECK_FAST_MESSAGE((((char*)p[i])[j] == i), "Error: data broken");
            }

            for (int i=0; i<NUM; i++) {
                p[i] = (char*)scalable_aligned_realloc(p[i], 2*allocSz[s], align[a]);
                CHECK(tbb::detail::is_aligned(p[i], align[a]));
                memset((char*)p[i]+allocSz[s], i+1, allocSz[s]);
            }
            for (int i=0; i<NUM; i++) {
                for (unsigned j=0; j<allocSz[s]; j++)
                    CHECK_FAST_MESSAGE((((char*)p[i])[j] == i), "Error: data broken");
                for (size_t j=allocSz[s]; j<2*allocSz[s]; j++)
                    CHECK_FAST_MESSAGE((((char*)p[i])[j] == i+1), "Error: data broken");
            }
            for (int i=0; i<NUM; i++)
                scalable_free(p[i]);
        }
}

#if __TBB_USE_ADDRESS_SANITIZER
//! \brief \ref error_guessing
TEST_CASE("Memory leaks test is not applicable under ASAN\n" * doctest::skip(true)) {}
#else
//! \brief \ref error_guessing
TEST_CASE("testing leaks") {
    // Check whether memory usage data can be obtained; if not, skip test_bootstrap_leak.
    if (utils::GetMemoryUsage()) {
        REQUIRE_MESSAGE(TestBootstrapLeak(), "Test failed");
    }
}
#endif // __TBB_USE_ADDRESS_SANITIZER

//! \brief \ref error_guessing
TEST_CASE("testing realloc mem size") {
    bool passed = true;
    // TestReallocMsize runs for each power of 2 and each Fibonacci number below 64K
    for (size_t a=1, b=1, sum=1; sum<=64*1024; ) {
        passed &= TestReallocMsize(sum);
        a = b;
        b = sum;
        sum = a+b;
    }
    for (size_t a=2; a<=64*1024; a*=2) {
        passed &= TestReallocMsize(a);
    }
    REQUIRE_MESSAGE( passed, "Test failed" );
}

//! \brief \ref error_guessing
TEST_CASE("testing memory align") {
    TestAlignedMsize();
}
