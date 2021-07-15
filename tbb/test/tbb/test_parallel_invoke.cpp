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

#include "common/test.h"
#include "common/utils.h"
#include "common/cpu_usertime.h"
#include "common/utils_concurrency_limit.h"
#include "common/parallel_invoke_common.h"
#include "common/memory_usage.h"

#include <cstddef>
#include <atomic>

//! \file test_parallel_invoke.cpp
//! \brief Test for [algorithms.parallel_invoke]

//! Testing parallel_invoke memory usage
//! \brief \ref resource_usage \ref stress
TEST_CASE("Test memory leaks") {
    std::size_t number_of_measurements = 500;
    std::size_t current_memory_usage = 0, max_memory_usage = 0, stability_counter=0;

    // Limit concurrency to prevent extra allocations not dependent on algorithm behavior
    auto concurrency_limit = utils::get_platform_max_threads() < 8 ? utils::get_platform_max_threads() : 8;
    tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_limit);

    for (std::size_t i = 0; i < number_of_measurements; i++) {
        {
            // ~45000 workload tasks
            invoke_tree</*LevelTaskCount*/6, /*Depth*/6, /*WorkSize*/10>::generate_and_run();
        }

        current_memory_usage = utils::GetMemoryUsage();
        if (current_memory_usage > max_memory_usage) {
            stability_counter = 0;
            max_memory_usage = current_memory_usage;
        } else {
            stability_counter++;
        }
        // If the amount of used memory has not changed during 10% of executions,
        // then we can assume that the check was successful
        if (stability_counter > number_of_measurements / 10) return;
    }
    REQUIRE_MESSAGE(false, "Seems like we get memory leak here.");
}

template<typename Body>
void test_from_2_to_10_arguments(const Body& body, const std::atomic<std::size_t>& counter) {
    tbb::parallel_invoke(body, body);
    tbb::parallel_invoke(body, body, body);
    tbb::parallel_invoke(body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body, body, body, body, body);
    tbb::parallel_invoke(body, body, body, body, body, body, body, body, body, body);

    REQUIRE_MESSAGE(counter == (2 + 10) * 9 / 2,
        "Parallel invoke correctness was broken during lambda support test execution.");
}

//! Testing lambdas support
//! \brief \ref error_guessing
TEST_CASE("Test lambda support") {
    std::atomic<std::size_t> lambda_counter{0};
    auto body = [&]{ lambda_counter++; };

    test_from_2_to_10_arguments(body, lambda_counter);
}

std::atomic<std::size_t> func_counter{0};
void func() { func_counter++; };

//! Testing function pointers support
//! \brief \ref error_guessing
TEST_CASE("Test function pointers support") {
    auto func_ptr = &func;
    test_from_2_to_10_arguments(func_ptr, func_counter);
}

//! Testing workers going to sleep
//! \brief \ref error_guessing
TEST_CASE("Test that all workers sleep when no work") {
    invoke_tree</*LevelTaskCount*/9, /*Depth*/6, /*WorkSize*/10>::generate_and_run();
    TestCPUUserTime(utils::get_platform_max_threads());
}
