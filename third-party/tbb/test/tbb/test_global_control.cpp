/*
    Copyright (c) 2005-2023 Intel Corporation

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

//! \file test_global_control.cpp
//! \brief Test for [sched.global_control] specification

#include "common/test.h"

#include "common/utils.h"
#include "common/spin_barrier.h"
#include "common/utils_concurrency_limit.h"

#include "tbb/global_control.h"
#include "tbb/parallel_for.h"
#include "tbb/task_group.h"
#include "tbb/task_arena.h"

#include <cstring>

struct task_scheduler_handle_guard {
    tbb::task_scheduler_handle m_handle{};

    task_scheduler_handle_guard() {
        m_handle = tbb::task_scheduler_handle{tbb::attach{}};
    }

    ~task_scheduler_handle_guard() {
        m_handle.release();
    }

    tbb::task_scheduler_handle& get() {
        return m_handle;
    }
};

namespace TestBlockingTerminateNS {

    struct TestAutoInitBody {
        void operator()( int ) const {
            tbb::parallel_for( 0, 100, utils::DummyBody() );
        }
    };

    static std::atomic<int> gSeed;
    static std::atomic<int> gNumSuccesses;

    class TestMultpleWaitBody {
        bool myAutoInit;
    public:
        TestMultpleWaitBody( bool autoInit = false ) : myAutoInit( autoInit ) {}
        void operator()( int ) const {
            task_scheduler_handle_guard init;
            if ( !myAutoInit ) {
                tbb::parallel_for(0, 10, utils::DummyBody());
            }
            utils::FastRandom<> rnd( ++gSeed );
            // In case of auto init sub-tests we skip
            //  - case #4 to avoid recursion
            //  - case #5 because it is explicit initialization
            const int numCases = myAutoInit ? 4 : 6;
            switch ( rnd.get() % numCases ) {
            case 0: {
                tbb::task_arena a;
                a.enqueue(utils::DummyBody() );
                break;
            }
            case 1: {
                tbb::task_group tg;
                utils::DummyBody eb;
                tg.run( eb );
                tg.wait();
                break;
            }
            case 2:
                tbb::parallel_for( 0, 100, utils::DummyBody() );
                break;
            case 3:
                /* do nothing */
                break;
            case 4:
                // Create and join several threads with auto initialized scheduler.
                utils::NativeParallelFor( rnd.get() % 5 + 1, TestMultpleWaitBody( true ) );
                break;
            case 5:
                {
                    task_scheduler_handle_guard init2;
                    bool res = tbb::finalize( init2.get(), std::nothrow );
                    REQUIRE( !res );
                }
                break;
            }
            if ( !myAutoInit && tbb::finalize( init.get(), std::nothrow ) )
                ++gNumSuccesses;
        }
    };

    void TestMultpleWait() {
        const int minThreads = 1;
        const int maxThreads = 16;
        const int numRepeats = 5;
        // Initialize seed with different values on different machines.
        gSeed = tbb::this_task_arena::max_concurrency();
        for ( int repeats = 0; repeats<numRepeats; ++repeats ) {
            for ( int threads = minThreads; threads<maxThreads; ++threads ) {
                gNumSuccesses = 0;
                utils::NativeParallelFor( threads, TestMultpleWaitBody() );
                REQUIRE_MESSAGE( gNumSuccesses > 0, "At least one blocking terminate must return 'true'" );
            }
        }
    }

#if TBB_USE_EXCEPTIONS
    template <typename F>
    void TestException( F &f ) {
        utils::suppress_unused_warning( f );
        bool caught = false;
        try {
            f();
            REQUIRE( false );
        }
        catch ( const tbb::unsafe_wait& e) {
            const char* msg = e.what();
            REQUIRE((msg && std::strlen(msg) != 0));
            caught = true;
        }
        catch ( ... ) {
            REQUIRE( false );
        }
        REQUIRE( caught );
    }

    class ExceptionTest1 {
        task_scheduler_handle_guard tsi1;
        int myIndex;

    public:
        ExceptionTest1( int index ) : myIndex( index ) {}

        void operator()() {
            task_scheduler_handle_guard tsi2;
            tbb::parallel_for(0, 2, utils::DummyBody()); // auto-init
            tbb::finalize((myIndex == 0 ? tsi1.get() : tsi2.get()));
            REQUIRE_MESSAGE( false, "Blocking terminate did not throw the exception" );
        }
    };

    struct ExceptionTest2 {
        class Body {
            utils::SpinBarrier& myBarrier;
        public:
            Body( utils::SpinBarrier& barrier ) : myBarrier( barrier ) {}
            void operator()( int ) const {
                myBarrier.wait();
                task_scheduler_handle_guard init;
                tbb::finalize( init.get() );
                REQUIRE_MESSAGE( false, "Blocking terminate did not throw the exception inside the parallel region" );
            }
        };
        void operator()() {
            const int numThreads = 4;
            tbb::global_control init(tbb::global_control::max_allowed_parallelism, numThreads);
            tbb::task_arena a(numThreads);
            a.execute([&] {
                utils::SpinBarrier barrier(numThreads);
                tbb::parallel_for(0, numThreads, Body(barrier));
                REQUIRE_MESSAGE(false, "Parallel loop did not throw the exception");
            });
        }
    };

    void TestExceptions() {
        ExceptionTest1 Test1(0);
        TestException( Test1 );
        ExceptionTest1 Test2(1);
        TestException( Test2 );
        if (utils::get_platform_max_threads() > 1) {
            // TODO: Fix the arena leak issue on single threaded machine
            // (see https://github.com/oneapi-src/oneTBB/issues/396)
            ExceptionTest2 Test3;
            TestException(Test3);
        }
    }

#endif /* TBB_USE_EXCEPTIONS */

} // namespace TestBlockingTerminateNS

void TestTerminationAndAutoinit(bool autoinit) {
    task_scheduler_handle_guard ctl1;
    task_scheduler_handle_guard ctl2;

    if (autoinit) {
        tbb::parallel_for(0, 10, utils::DummyBody());
    }
    bool res1 = tbb::finalize(ctl1.get(), std::nothrow);
    if (autoinit) {
        REQUIRE(!res1);
    } else {
        REQUIRE(res1);
    }
    bool res2 = tbb::finalize(ctl2.get(), std::nothrow);
    REQUIRE(res2);
}

//! Check no reference leak for an external thread
//! \brief \ref regression \ref error_guessing
TEST_CASE("test decrease reference") {
    tbb::task_scheduler_handle handle = tbb::task_scheduler_handle{tbb::attach{}};

    std::thread thr([] { tbb::parallel_for(0, 1, [](int) {}); } );
    thr.join();

    REQUIRE(tbb::finalize(handle, std::nothrow));
}

//! Testing lifetime control
//! \brief \ref error_guessing
TEST_CASE("prolong lifetime auto init") {
    TestTerminationAndAutoinit(false);
    TestTerminationAndAutoinit(true);
}

#if TBB_USE_EXCEPTIONS
//! Testing lifetime control advanced
//! \brief \ref error_guessing
TEST_CASE("prolong lifetime advanced") {
    // Exceptions test leaves auto-initialized scheduler after,
    // because all blocking terminate calls are inside the parallel region,
    // thus resulting in false termination result.
    utils::NativeParallelFor(1,
        [&](int) { TestBlockingTerminateNS::TestExceptions(); });
}
#endif

//! Testing multiple wait
//! \brief \ref error_guessing
TEST_CASE("prolong lifetime multiple wait") {
    TestBlockingTerminateNS::TestMultpleWait();
}

//! \brief \ref regression
TEST_CASE("test concurrent task_scheduler_handle destruction") {
    std::atomic<bool> stop{ false };
    std::thread thr1([&] {
        while (!stop) {
            auto h = tbb::task_scheduler_handle{ tbb::attach{} };
            tbb::finalize(h, std::nothrow_t{});
        }
    });

    for (int i = 0; i < 1000; ++i) {
        std::thread thr2([] {
            tbb::parallel_for(0, 1, [](int) {});
        });
        thr2.join();
    }
    stop = true;
    thr1.join();
}
