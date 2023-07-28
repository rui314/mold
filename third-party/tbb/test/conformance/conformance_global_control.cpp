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
#include "common/test.h"
#include "common/concurrency_tracker.h"
#include "common/spin_barrier.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"

#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/parallel_for.h"

#include <limits.h>
#include <thread>

//! \file conformance_global_control.cpp
//! \brief Test for [sched.global_control] specification

const std::size_t MB = 1024*1024;

void TestStackSizeSimpleControl() {
    oneapi::tbb::global_control s0(oneapi::tbb::global_control::thread_stack_size, 1*MB);

    {
        oneapi::tbb::global_control s1(oneapi::tbb::global_control::thread_stack_size, 8*MB);

        CHECK(8*MB == oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::thread_stack_size));
    }
    CHECK(1*MB == oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::thread_stack_size));
}

struct StackSizeRun : utils::NoAssign {

    int num_threads;
    utils::SpinBarrier *barr1, *barr2;

    StackSizeRun(int threads, utils::SpinBarrier *b1, utils::SpinBarrier *b2) :
        num_threads(threads), barr1(b1), barr2(b2) {}
    void operator()( int id ) const {
        oneapi::tbb::global_control s1(oneapi::tbb::global_control::thread_stack_size, (1+id)*MB);

        barr1->wait();

        REQUIRE(num_threads*MB == oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::thread_stack_size));
        barr2->wait();
    }
};

void TestStackSizeThreadsControl() {
    int threads = 4;
    utils::SpinBarrier barr1(threads), barr2(threads);
    utils::NativeParallelFor( threads, StackSizeRun(threads, &barr1, &barr2) );
}

void RunWorkersLimited(size_t parallelism, bool wait)
{
    oneapi::tbb::global_control s(oneapi::tbb::global_control::max_allowed_parallelism, parallelism);
    // try both configuration with already sleeping workers and with not yet sleeping
    if (wait)
        utils::Sleep(10);
    const std::size_t expected_threads = (utils::get_platform_max_threads()==1)? 1 : parallelism;
    utils::ExactConcurrencyLevel::check(expected_threads);
}

void TestWorkersConstraints()
{
    const size_t max_parallelism =
        oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
    if (max_parallelism > 3) {
        oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, max_parallelism-1);
        CHECK_MESSAGE(max_parallelism-1 ==
               oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism),
               "Allowed parallelism must be decreasable.");
        oneapi::tbb::global_control c1(oneapi::tbb::global_control::max_allowed_parallelism, max_parallelism-2);
        CHECK_MESSAGE(max_parallelism-2 ==
               oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism),
               "Allowed parallelism must be decreasable.");
    }
    const size_t limit_par = utils::min(max_parallelism, 4U);
    // check that constrains are really met
    for (int wait=0; wait<2; wait++) {
        for (size_t num=2; num<limit_par; num++)
            RunWorkersLimited(num, wait==1);
        for (size_t num=limit_par; num>1; num--)
            RunWorkersLimited(num, wait==1);
    }
}

struct SetUseRun: utils::NoAssign {
    utils::SpinBarrier &barr;

    SetUseRun(utils::SpinBarrier& b) : barr(b) {}
    void operator()( int id ) const {
        if (id == 0) {
            for (int i=0; i<10; i++) {
                oneapi::tbb::parallel_for(0, 1000, utils::DummyBody(10), oneapi::tbb::simple_partitioner());
                barr.wait();
            }
        } else {
            for (int i=0; i<10; i++) {
                oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, 8);
                barr.wait();
            }
        }
    }
};

void TestConcurrentSetUseConcurrency()
{
    utils::SpinBarrier barr(2);
    NativeParallelFor( 2, SetUseRun(barr) );
}

// check number of workers after autoinitialization
void TestAutoInit()
{
    const size_t max_parallelism =
        oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
    const unsigned expected_threads = utils::get_platform_max_threads()==1?
        1 : (unsigned)max_parallelism;
    utils::ExactConcurrencyLevel::check(expected_threads);
    CHECK_MESSAGE(oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism)
           == max_parallelism, "max_allowed_parallelism must not be changed after auto init");
    if (max_parallelism > 2) {
        // after autoinit it's possible to decrease workers number
        oneapi::tbb::global_control s(oneapi::tbb::global_control::max_allowed_parallelism, max_parallelism-1);
        utils::ExactConcurrencyLevel::check(max_parallelism-1);
    }
}

class TestMultipleControlsRun {
    utils::SpinBarrier &barrier;
public:
    TestMultipleControlsRun(utils::SpinBarrier& b) : barrier(b) {}
    void operator()( int id ) const {
        barrier.wait();
        if (id) {
            {
                oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, 1);
                utils::ExactConcurrencyLevel::check(1);
                barrier.wait();
            }
            utils::ExactConcurrencyLevel::check(1);
            barrier.wait();
            {
                oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, 2);
                utils::ExactConcurrencyLevel::check(1);
                barrier.wait();
                utils::ExactConcurrencyLevel::check(2);
                barrier.wait();
            }
        } else {
            {
                utils::ExactConcurrencyLevel::check(1);
                oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, 1);
                barrier.wait();
                utils::ExactConcurrencyLevel::check(1);
                barrier.wait();
                utils::ExactConcurrencyLevel::check(1);
                barrier.wait();
            }
            utils::ExactConcurrencyLevel::check(2);
            barrier.wait();
        }
    }
};

// test that global controls from different thread with overlapping lifetime
// still keep parallelism under control
void TestMultipleControls()
{
    utils::SpinBarrier barrier(2);
    utils::NativeParallelFor( 2, TestMultipleControlsRun(barrier) );
}

#if !(__TBB_WIN8UI_SUPPORT && (_WIN32_WINNT < 0x0A00))
//! Testing setting stack size
//! \brief \ref interface \ref requirement
TEST_CASE("setting stack size") {
    std::size_t default_ss = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::thread_stack_size);
    CHECK(default_ss > 0);
    TestStackSizeSimpleControl();
    TestStackSizeThreadsControl();
    CHECK(default_ss == oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::thread_stack_size));
}
#endif

//! Testing setting max number of threads
//! \brief \ref interface \ref requirement
TEST_CASE("setting max number of threads") {
    TestWorkersConstraints();
    TestConcurrentSetUseConcurrency();
    TestAutoInit();
}

//! Test terminate_on_exception default value
//! \brief \ref interface \ref requirement
TEST_CASE("terminate_on_exception: default") {
    std::size_t default_toe = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::terminate_on_exception);
    CHECK(default_toe == 0);
}

//! Test terminate_on_exception in a nested case
//! \brief \ref interface \ref requirement
TEST_CASE("terminate_on_exception: nested") {
    oneapi::tbb::global_control* c0;
    {
        oneapi::tbb::global_control c1(oneapi::tbb::global_control::terminate_on_exception, 1);
        CHECK(oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::terminate_on_exception) == 1);
        {
            oneapi::tbb::global_control c2(oneapi::tbb::global_control::terminate_on_exception, 0);
            CHECK(oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::terminate_on_exception) == 1);
        }
        CHECK(oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::terminate_on_exception) == 1);
        c0 = new oneapi::tbb::global_control(oneapi::tbb::global_control::terminate_on_exception, 0);
    }
    CHECK(oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::terminate_on_exception) == 0);
    delete c0;
}

//! Testing setting the same value but different objects
//! \brief \ref interface \ref error_guessing
TEST_CASE("setting same value") {
    const std::size_t value = 2;

    oneapi::tbb::global_control* ctl1 = new oneapi::tbb::global_control(oneapi::tbb::global_control::max_allowed_parallelism, value);
    oneapi::tbb::global_control* ctl2 = new oneapi::tbb::global_control(oneapi::tbb::global_control::max_allowed_parallelism, value);

    std::size_t active = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
    REQUIRE(active == value);
    delete ctl2;

    active = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
    REQUIRE_MESSAGE(active == value, "Active value should not change, because of value duplication");
    delete ctl1;
}

//! Testing lifetime control conformance
//! \brief \ref interface \ref requirement
TEST_CASE("prolong lifetime simple") {
    tbb::task_scheduler_handle hdl1{ tbb::attach{} };
    {
        tbb::parallel_for(0, 10, utils::DummyBody());

        tbb::task_scheduler_handle hdl2;
        hdl2 = tbb::task_scheduler_handle{ tbb::attach{} };
        hdl2.release();
    }
    bool ok = tbb::finalize(hdl1, std::nothrow);
    REQUIRE(ok);
}

//! Testing handle check for emptiness
//! \brief \ref interface \ref requirement
TEST_CASE("null handle check") {
    tbb::task_scheduler_handle hndl;
    REQUIRE_FALSE(hndl);
}

//! Testing handle check for emptiness
//! \brief \ref interface \ref requirement
TEST_CASE("null handle check 2") {
    tbb::task_scheduler_handle hndl{ tbb::attach{} };
    bool not_empty = (bool)hndl;

    tbb::finalize(hndl, std::nothrow);

    REQUIRE(not_empty);
    REQUIRE_FALSE(hndl);
}

//! Testing handle check for emptiness
//! \brief \ref interface \ref requirement
TEST_CASE("null handle check 3") {
    tbb::task_scheduler_handle handle1{ tbb::attach{} };
    tbb::task_scheduler_handle handle2(std::move(handle1));

    bool handle1_empty = !handle1;
    bool handle2_not_empty = (bool)handle2;

    tbb::finalize(handle2, std::nothrow);

    REQUIRE(handle1_empty);
    REQUIRE(handle2_not_empty);
}

//! Testing  task_scheduler_handle is created on one thread and destroyed on another.
//! \brief \ref interface \ref requirement
TEST_CASE("cross thread 1") {
    // created task_scheduler_handle, parallel_for on another thread - finalize
    tbb::task_scheduler_handle handle{ tbb::attach{} };
    utils::NativeParallelFor(1, [&](int) {
        tbb::parallel_for(0, 10, utils::DummyBody());
        bool res = tbb::finalize(handle, std::nothrow);
        REQUIRE(res);
    });
}

//! Testing  task_scheduler_handle is created on one thread and destroyed on another.
//! \brief \ref interface \ref requirement
TEST_CASE("cross thread 2") {
    // created task_scheduler_handle, called parallel_for on this thread, killed the thread - and finalize on another thread
    tbb::task_scheduler_handle handle;
    utils::NativeParallelFor(1, [&](int) {
        handle = tbb::task_scheduler_handle{ tbb::attach{} };
        tbb::parallel_for(0, 10, utils::DummyBody());
    });
    bool res = tbb::finalize(handle, std::nothrow);
    REQUIRE(res);
}

//! Testing multiple wait
//! \brief \ref interface \ref requirement
TEST_CASE("simple prolong lifetime 3") {
    // Parallel region
    tbb::parallel_for(0, 10, utils::DummyBody());
    // Termination
    tbb::task_scheduler_handle handle = tbb::task_scheduler_handle{ tbb::attach{} };
    bool res = tbb::finalize(handle, std::nothrow);
    REQUIRE(res);
    // New parallel region
    tbb::parallel_for(0, 10, utils::DummyBody());
}

// The test cannot work correctly with statically linked runtime.
// TODO: investigate a failure in debug with MSVC
#if !_MSC_VER || (defined(_DLL) && !defined(_DEBUG))
#include <csetjmp>

// Overall, the test case is not safe because the dtors might not be called during long jump.
// Therefore, it makes sense to run the test case after all other test cases.
//! Test terminate_on_exception behavior
//! \brief \ref interface \ref requirement
TEST_CASE("terminate_on_exception: enabled") {
    oneapi::tbb::global_control c(oneapi::tbb::global_control::terminate_on_exception, 1);
    static bool terminate_handler_called;
    terminate_handler_called = false;

#if TBB_USE_EXCEPTIONS
    try {
#endif
        static std::jmp_buf buffer;
        std::terminate_handler prev = std::set_terminate([] {
            CHECK(!terminate_handler_called);
            terminate_handler_called = true;
            std::longjmp(buffer, 1);
        });
#if _MSC_VER
#pragma warning(push)
#pragma warning(disable:4611) // interaction between '_setjmp' and C++ object destruction is non - portable
#endif
        SUBCASE("internal exception") {
            if (setjmp(buffer) == 0) {
                oneapi::tbb::parallel_for(0, 1, -1, [](int) {});
                FAIL("Unreachable code");
            }
        }
#if TBB_USE_EXCEPTIONS
        SUBCASE("user exception") {
            if (setjmp(buffer) == 0) {
                oneapi::tbb::parallel_for(0, 1, [](int) {
                    volatile bool suppress_unreachable_code_warning = true;
                    if (suppress_unreachable_code_warning) {
                        throw std::exception();
                    }
                });
                FAIL("Unreachable code");
            }
        }
#endif
#if _MSC_VER
#pragma warning(pop)
#endif
        std::set_terminate(prev);
        terminate_handler_called = true;
#if TBB_USE_EXCEPTIONS
    } catch (...) {
        FAIL("The exception is not expected");
    }
#endif
    CHECK(terminate_handler_called);
}
#endif
