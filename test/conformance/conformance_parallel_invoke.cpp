/*
    Copyright (c) 2020-2021 Intel Corporation

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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif


#include "common/test.h"
#include "common/utils.h"
#include "common/cpu_usertime.h"
#include "common/utils_concurrency_limit.h"
#include "common/parallel_invoke_common.h"
#include "common/memory_usage.h"

#include <cstddef>
#include <cstdint>
#include <atomic>

//! \file conformance_parallel_invoke.cpp
//! \brief Test for [algorithms.parallel_invoke] specification

template<std::size_t TaskCount>
struct correctness_test_case {
    static std::atomic<std::size_t> data_array[TaskCount];

    // invocation functor
    template<std::size_t Position>
    struct functor {

        functor() = default;
        functor(const functor&) = delete;
        functor& operator=(const functor&) = delete;

        void operator()() const {
            REQUIRE_MESSAGE(Position < TaskCount, "Wrong structure configuration.");
            data_array[Position]++;
        }
    };

    static void run_validate_and_reset(oneapi::tbb::task_group_context* context_ptr) {
        for (auto& elem : data_array)
            elem.store(0, std::memory_order_relaxed);

        parallel_invoke_call<TaskCount, functor>::perform(context_ptr);
        for (std::size_t i = 0; i < TaskCount; i++) {
            REQUIRE_MESSAGE(data_array[i] == 1, "Some task was executed more than once, or was not executed.");
            data_array[i] = 0;
        }
    }
};

template<std::size_t TaskCount>
std::atomic<std::size_t> correctness_test_case<TaskCount>::data_array[TaskCount];

void correctness_test(oneapi::tbb::task_group_context* context_ptr = nullptr) {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        correctness_test_case<2>::run_validate_and_reset(context_ptr);
        correctness_test_case<3>::run_validate_and_reset(context_ptr);
        correctness_test_case<4>::run_validate_and_reset(context_ptr);
        correctness_test_case<5>::run_validate_and_reset(context_ptr);
        correctness_test_case<6>::run_validate_and_reset(context_ptr);
        correctness_test_case<7>::run_validate_and_reset(context_ptr);
        correctness_test_case<8>::run_validate_and_reset(context_ptr);
        correctness_test_case<9>::run_validate_and_reset(context_ptr);
        correctness_test_case<10>::run_validate_and_reset(context_ptr);
        // ...
        correctness_test_case<50>::run_validate_and_reset(context_ptr);
        correctness_test_case<51>::run_validate_and_reset(context_ptr);
        correctness_test_case<52>::run_validate_and_reset(context_ptr);
    }
}

//! Testing correctness with various functors count
//! \brief \ref requirement \ref interface
TEST_CASE("Test correctness") {
    correctness_test();
}

//! Testing correctness with various functors count using task_group_context
//! \brief \ref requirement \ref interface
TEST_CASE("Test correctness using context") {
    oneapi::tbb::task_group_context context;
    correctness_test(&context);
}

// Exception handling support test
#define UTILS_EXCEPTION_HANDLING_SIMPLE_MODE 1
#include "common/exception_handling.h"

#if TBB_USE_EXCEPTIONS

template<std::size_t TaskCount>
struct exception_handling_test_case {
    // invocation functor
    template<std::size_t Position>
    struct functor {
        functor() = default;
        functor(const functor&) = delete;
        functor& operator=(const functor&) = delete;

        void operator()() const {
            REQUIRE_MESSAGE(Position < TaskCount, "Wrong structure configuration.");
            if (exception_mask & (1 << Position)) {
                ThrowTestException();
            }
        }
    };

    static void run_validate_and_reset() {
        // Checks all permutations of the exception handling mask for the current tasks count
        for( exception_mask = 1; exception_mask < (std::size_t(1) << TaskCount); ++exception_mask ) {
            ResetEhGlobals();
            TRY();
                parallel_invoke_call<TaskCount, functor>::perform();
            CATCH();
            ASSERT_EXCEPTION();
        }
    }
private:
    static std::uint64_t exception_mask; // each bit represents whether the function should throw exception or not
};

template<std::size_t TaskCount>
std::uint64_t exception_handling_test_case<TaskCount>::exception_mask(0);

//! Testing exception hangling
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Test exception hangling") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        if (concurrency_level < 2) continue;
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        exception_handling_test_case<2>::run_validate_and_reset();
        exception_handling_test_case<3>::run_validate_and_reset();
        exception_handling_test_case<4>::run_validate_and_reset();
        exception_handling_test_case<5>::run_validate_and_reset();
        exception_handling_test_case<6>::run_validate_and_reset();
        exception_handling_test_case<7>::run_validate_and_reset();
        exception_handling_test_case<8>::run_validate_and_reset();
        exception_handling_test_case<9>::run_validate_and_reset();
        exception_handling_test_case<10>::run_validate_and_reset();
    }
}
#endif /* TBB_USE_EXCEPTIONS */

// Cancellation support test
void function_to_cancel() {
    ++g_CurExecuted;
    Cancellator::WaitUntilReady();
}

// The function is used to test cancellation
void simple_test_nothrow (){
    ++g_CurExecuted;
}

std::size_t g_numFunctions, g_functionToCancel;

struct ParInvokeLauncher {
    oneapi::tbb::task_group_context &my_ctx;

    void operator()() const {
        void(*func_array[10])(void);
        for (int i = 0; i <=9; ++i)
            func_array[i] = &simple_test_nothrow;
        func_array[g_functionToCancel] = &function_to_cancel;

        oneapi::tbb::parallel_invoke(func_array[0], func_array[1], func_array[2], func_array[3],
            func_array[4], func_array[5], func_array[6], func_array[7], func_array[8], func_array[9], my_ctx);
    }

    ParInvokeLauncher ( oneapi::tbb::task_group_context& ctx ) : my_ctx(ctx) {}
};

//! Testing cancellation
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Test cancellation") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        if (concurrency_level < 2) continue;
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        for ( int n = 2; n <= 10; ++n ) {
            for ( int m = 0; m <= n - 1; ++m ) {
                g_numFunctions = n;
                g_functionToCancel = m;
                ResetEhGlobals();
                RunCancellationTest<ParInvokeLauncher, Cancellator>();
            }
        }
    }
}
