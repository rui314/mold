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
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"

#include <tbb/tick_count.h>
#include <thread>

//! \file test_tick_count.cpp
//! \brief Test for [timing] specification

//! Testing clock type of tbb::tick_count
//! Clock in tbb::tick_count should be steady
//! \brief \ref requirement
TEST_CASE("Clock in tbb::tick_count should be steady") {
    CHECK_EQ(tbb::tick_count::clock_type::is_steady, true);
}

#if TBB_USE_EXCEPTIONS
//! Subtraction of equal tick_counts should not throw
//! \brief \ref error_guessing
TEST_CASE("Subtraction of equal tick_counts should not throw") {
    tbb::tick_count tick_f = tbb::tick_count::now();
    tbb::tick_count tick_s(tick_f);
    CHECK_NOTHROW(tick_f - tick_s);
}
#endif

//! Test that two tick_count values recorded on different threads can be meaningfully subtracted.
//! \brief \ref error_guessing
TEST_CASE("Test for subtracting calls to tick_count from different threads") {
    auto num_of_threads = utils::get_platform_max_threads();

    utils::SpinBarrier thread_barrier(num_of_threads);
    tbb::tick_count start_time;

    auto diff_func = [&thread_barrier, &start_time] (std::size_t ) {
        thread_barrier.wait([&start_time] { start_time = tbb::tick_count::now(); });

        tbb::tick_count end_time(tbb::tick_count::now());
        while ((end_time - start_time).seconds() == 0) {
            end_time = tbb::tick_count::now();
        }

        CHECK_GT((end_time - start_time).seconds(), 0);
    };

    for (std::size_t i = 0; i < 10; ++i) {
        utils::NativeParallelFor(num_of_threads, diff_func);
    }
}
