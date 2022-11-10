/*
    Copyright (c) 2021-2022 Intel Corporation

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

#include "oneapi/tbb/task_group.h"
#include "oneapi/tbb/task_arena.h"
#include "common/test.h"
#include "common/utils.h"

#include "common/spin_barrier.h"

#include <type_traits>
#include "oneapi/tbb/global_control.h"


//! \file conformance_task_group.cpp
//! \brief Test for [scheduler.task_group] specification

//! Test checks that for lost task handle
//! \brief \ref requirement
TEST_CASE("Task handle created but not run"){
    {
        oneapi::tbb::task_group tg;

        //This flag is intentionally made non-atomic for Thread Sanitizer
        //to raise a flag if implementation of task_group is incorrect
        bool run {false};

        auto h = tg.defer([&]{
            run = true;
        });
        CHECK_MESSAGE(run == false, "delayed task should not be run until run(task_handle) is called");
    }
}

//! Basic test for task handle
//! \brief \ref interface \ref requirement
TEST_CASE("Task handle run"){
    oneapi::tbb::task_handle h;

    oneapi::tbb::task_group tg;
    //This flag is intentionally made non-atomic for Thread Sanitizer
    //to raise a flag if implementation of task_group is incorrect
    bool run {false};

    h = tg.defer([&]{
        run = true;
    });
    CHECK_MESSAGE(run == false, "delayed task should not be run until run(task_handle) is called");
    tg.run(std::move(h));
    tg.wait();
    CHECK_MESSAGE(run == true, "Delayed task should be completed when task_group::wait exits");

    CHECK_MESSAGE(h == nullptr, "Delayed task can be executed only once");
}

//! Basic test for task handle
//! \brief \ref interface \ref requirement
TEST_CASE("Task handle run_and_wait"){
    oneapi::tbb::task_handle h;

    oneapi::tbb::task_group tg;
    //This flag is intentionally made non-atomic for Thread Sanitizer
    //to raise a flag if implementation of task_group is incorrect
    bool run {false};

    h = tg.defer([&]{
        run = true;
    });
    CHECK_MESSAGE(run == false, "delayed task should not be run until run(task_handle) is called");
    tg.run_and_wait(std::move(h));
    CHECK_MESSAGE(run == true, "Delayed task should be completed when task_group::wait exits");

    CHECK_MESSAGE(h == nullptr, "Delayed task can be executed only once");
}

//! Test for empty check
//! \brief \ref interface
TEST_CASE("Task handle empty check"){
    oneapi::tbb::task_group tg;

    oneapi::tbb::task_handle h;

    bool empty = (h == nullptr);
    CHECK_MESSAGE(empty, "default constructed task_handle should be empty");

    h = tg.defer([]{});

    CHECK_MESSAGE(h != nullptr, "delayed task returned by task_group::delayed should not be empty");
}

//! Test for comparison operations
//! \brief \ref interface
TEST_CASE("Task handle comparison/empty checks"){
    oneapi::tbb::task_group tg;

    oneapi::tbb::task_handle h;

    bool empty =  ! static_cast<bool>(h);
    CHECK_MESSAGE(empty, "default constructed task_handle should be empty");
    CHECK_MESSAGE(h == nullptr, "default constructed task_handle should be empty");
    CHECK_MESSAGE(nullptr == h, "default constructed task_handle should be empty");

    h = tg.defer([]{});

    CHECK_MESSAGE(h != nullptr, "deferred task returned by task_group::defer() should not be empty");
    CHECK_MESSAGE(nullptr != h, "deferred task returned by task_group::defer() should not be empty");

}

//! Test for task_handle being non copyable
//! \brief \ref interface
TEST_CASE("Task handle being non copyable"){
    static_assert(
              (!std::is_copy_constructible<oneapi::tbb::task_handle>::value)
            &&(!std::is_copy_assignable<oneapi::tbb::task_handle>::value),
            "oneapi::tbb::task_handle should be non copyable");
}
//! Test that task_handle prolongs task_group::wait
//! \brief \ref requirement
TEST_CASE("Task handle blocks wait"){
    //Forbid creation of worker threads to ensure that task described by the task_handle is not run until wait is called
    oneapi::tbb::global_control s(oneapi::tbb::global_control::max_allowed_parallelism, 1);
    oneapi::tbb::task_group tg;
    //explicit task_arena is needed to prevent a deadlock,
    //as both task_group::run() and task_group::wait() should be called in the same arena
    //to guarantee execution of the task spawned by run().
    oneapi::tbb::task_arena arena;

    //This flag is intentionally made non-atomic for Thread Sanitizer
    //to raise a flag if implementation of task_group is incorrect
    bool completed  {false};
    std::atomic<bool> start_wait {false};
    std::atomic<bool> thread_started{false};

    oneapi::tbb::task_handle h = tg.defer([&]{
        completed = true;
    });

    std::thread wait_thread {[&]{
        CHECK_MESSAGE(completed == false, "Deferred task should not be run until run(task_handle) is called");

        thread_started = true;
        utils::SpinWaitUntilEq(start_wait, true);
        arena.execute([&]{
            tg.wait();
            CHECK_MESSAGE(completed == true, "Deferred task should be completed when task_group::wait exits");
        });
    }};

    utils::SpinWaitUntilEq(thread_started, true);
    CHECK_MESSAGE(completed == false, "Deferred task should not be run until run(task_handle) is called");

    arena.execute([&]{
        tg.run(std::move(h));
    });
    CHECK_MESSAGE(completed == false, "Deferred task should not be run until run(task_handle) and wait is called");
    start_wait = true;
    wait_thread.join();
}

#if TBB_USE_EXCEPTIONS
//! The test for exception handling in task_handle
//! \brief \ref requirement
TEST_CASE("Task handle exception propagation"){
    oneapi::tbb::task_group tg;

    oneapi::tbb::task_handle h = tg.defer([&]{
        volatile bool suppress_unreachable_code_warning = true;
        if (suppress_unreachable_code_warning) {
            throw std::runtime_error{ "" };
        }
    });

    tg.run(std::move(h));

    CHECK_THROWS_AS(tg.wait(), std::runtime_error);
}
#endif // TBB_USE_EXCEPTIONS

//TODO: move to some common place to share with unit tests
namespace accept_task_group_context {

struct SelfRunner {
    tbb::task_group& m_tg;
    std::atomic<unsigned>& count;
    void operator()() const {
        unsigned previous_count = count.fetch_sub(1);
        if (previous_count > 1)
            m_tg.run( *this );
    }
};

template <typename TaskGroup, typename CancelF, typename WaitF>
void run_cancellation_use_case(CancelF&& cancel, WaitF&& wait) {
    std::atomic<bool> outer_cancelled{false};
    std::atomic<unsigned> count{13};

    tbb::task_group_context inner_ctx(tbb::task_group_context::isolated);
    TaskGroup inner_tg(inner_ctx);

    tbb::task_group outer_tg;
    auto outer_tg_task = [&] {
        inner_tg.run([&] {
            utils::SpinWaitUntilEq(outer_cancelled, true);
            inner_tg.run( SelfRunner{inner_tg, count} );
        });

        utils::try_call([&] {
            std::forward<CancelF>(cancel)(outer_tg);
        }).on_completion([&] {
            outer_cancelled = true;
        });
    };

    auto check = [&] {
        tbb::task_group_status outer_status = tbb::task_group_status::not_complete;
        outer_status = std::forward<WaitF>(wait)(outer_tg);
        CHECK_MESSAGE(
            outer_status == tbb::task_group_status::canceled,
            "Outer task group should have been cancelled."
        );

        tbb::task_group_status inner_status = inner_tg.wait();
        CHECK_MESSAGE(
            inner_status == tbb::task_group_status::complete,
            "Inner task group should have completed despite the cancellation of the outer one."
        );

        CHECK_MESSAGE(0 == count, "Some of the inner group tasks were not executed.");
    };

    outer_tg.run(outer_tg_task);
    check();
}

template <typename TaskGroup>
void test() {
    run_cancellation_use_case<TaskGroup>(
        [](tbb::task_group& outer) { outer.cancel(); },
        [](tbb::task_group& outer) { return outer.wait(); }
    );

#if TBB_USE_EXCEPTIONS
    run_cancellation_use_case<TaskGroup>(
        [](tbb::task_group& /*outer*/) {
            volatile bool suppress_unreachable_code_warning = true;
            if (suppress_unreachable_code_warning) {
                throw int();
            }
        },
        [](tbb::task_group& outer) {
            try {
                outer.wait();
                return tbb::task_group_status::complete;
            } catch(const int&) {
                return tbb::task_group_status::canceled;
            }
        }
    );
#endif
}

} // namespace accept_task_group_context

//! Respect task_group_context passed from outside
//! \brief \ref interface \ref requirement
TEST_CASE("Respect task_group_context passed from outside") {
    accept_task_group_context::test<tbb::task_group>();
}

