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
#include "common/spin_barrier.h"
#include "oneapi/tbb/detail/_utils.h"
#include "tbb/scalable_allocator.h"
#include <thread>

static constexpr std::size_t MaxTasks = 16;
std::atomic<std::size_t> FinishedTasks;

static constexpr std::size_t MaxThread = 4;

/*--------------------------------------------------------------------*/
// The regression test against a bug triggered when malloc initialization
// and thread shutdown were called simultaneously, in which case
// Windows dynamic loader lock and allocator initialization/termination lock
// were taken in different order.



class TestFunc1 {
    utils::SpinBarrier* my_barr;
public:
    TestFunc1 (utils::SpinBarrier& barr) : my_barr(&barr) {}
    void operator() (bool do_malloc) const {
        my_barr->wait();
        if (do_malloc) scalable_malloc(10);
        ++FinishedTasks;
    }
};

void Test1 () {
    std::size_t NTasks = utils::min(MaxTasks, utils::max(std::size_t(2), MaxThread));
    utils::SpinBarrier barr(NTasks);
    TestFunc1 tf(barr);
    FinishedTasks = 0;

    utils::NativeParallelFor(NTasks, [&] (std::size_t thread_idx) {
        tf(thread_idx % 2 == 0);
        while (FinishedTasks != NTasks) utils::yield();
    });
}

/*--------------------------------------------------------------------*/
// The regression test against a bug when cross-thread deallocation
// caused livelock at thread shutdown.

std::atomic<void*> gPtr(nullptr);

class TestFunc2a {
    utils::SpinBarrier* my_barr;
public:
    TestFunc2a (utils::SpinBarrier& barr) : my_barr(&barr) {}
    void operator() (std::size_t) const {
        gPtr = scalable_malloc(8);
        my_barr->wait();
        ++FinishedTasks;
    }
};

class TestFunc2b {
    utils::SpinBarrier* my_barr;
    std::thread& my_ward;
public:
    TestFunc2b (utils::SpinBarrier& barr, std::thread& t) : my_barr(&barr), my_ward(t) {}
    void operator() (std::size_t) const {
        utils::SpinWaitWhileEq(gPtr, (void*)nullptr);
        scalable_free(gPtr);
        my_barr->wait();
        my_ward.join();
        ++FinishedTasks;
    }
};
void Test2() {
    utils::SpinBarrier barr(2);
    TestFunc2a func2a(barr);
    std::thread t2a;
    TestFunc2b func2b(barr, t2a);
    FinishedTasks = 0;
    t2a = std::thread(func2a, std::size_t(0));
    std::thread t2b(func2b, std::size_t(1));

    while (FinishedTasks != 2) utils::yield();

    t2b.join(); // t2a is monitored by t2b

    if (t2a.joinable()) t2a.join();
}

#if _WIN32||_WIN64

void TestKeyDtor() {}

#else

void *currSmall, *prevSmall, *currLarge, *prevLarge;

extern "C" void threadDtor(void*) {
    // First, release memory that was allocated before;
    // it will not re-initialize the thread-local data if already deleted
    prevSmall = currSmall;
    scalable_free(currSmall);
    prevLarge = currLarge;
    scalable_free(currLarge);
    // Then, allocate more memory.
    // It will re-initialize the allocator data in the thread.
    scalable_free(scalable_malloc(8));
}

inline bool intersectingObjects(const void *p1, const void *p2, size_t n)
{
    return p1>p2 ? ((uintptr_t)p1-(uintptr_t)p2)<n : ((uintptr_t)p2-(uintptr_t)p1)<n;
}

struct TestThread: utils::NoAssign {
    TestThread(int ) {}

    void operator()( std::size_t /*id*/ ) const {
        pthread_key_t key;

        currSmall = scalable_malloc(8);
        REQUIRE_MESSAGE((!prevSmall || currSmall==prevSmall), "Possible memory leak");
        currLarge = scalable_malloc(32*1024);
        // intersectingObjects takes into account object shuffle
        REQUIRE_MESSAGE((!prevLarge || intersectingObjects(currLarge, prevLarge, 32*1024)), "Possible memory leak");
        pthread_key_create( &key, &threadDtor );
        pthread_setspecific(key, (const void*)this);
    }
};

// test releasing memory from pthread key destructor
void TestKeyDtor() {
    // Allocate region for large objects to prevent whole region release
    // on scalable_free(currLarge) call, which result in wrong assert inside intersectingObjects check
    void* preventLargeRelease = scalable_malloc(32*1024);
    for (int i=0; i<4; i++)
        utils::NativeParallelFor( 1, TestThread(1) );
    scalable_free(preventLargeRelease);
}

#endif // _WIN32||_WIN64


//! \brief \ref error_guessing
TEST_CASE("test1") {
    Test1(); // requires malloc initialization so should be first
}

//! \brief \ref error_guessing
TEST_CASE("test2") {
    Test2();
}

//! \brief \ref error_guessing
TEST_CASE("test key dtor") {
    TestKeyDtor();
}
