/*
    Copyright (c) 2021 Intel Corporation

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

#include "tbb/parallel_for.h"
#include "tbb/task_arena.h"
#include "tbb/global_control.h"

#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/dummy_body.h"
#include "common/spin_barrier.h"

#include <cstddef>
#include <utility>
#include <vector>

//! \file test_partitioner.cpp
//! \brief Test for [internal] functionality

namespace task_affinity_retention {

template <typename PerBodyFunc> float test(PerBodyFunc&& body) {
    const std::size_t num_threads = 2 * utils::get_platform_max_threads();
    tbb::global_control concurrency(tbb::global_control::max_allowed_parallelism, num_threads);
    tbb::task_arena big_arena(static_cast<int>(num_threads));

    const std::size_t repeats = 100;
    const std::size_t per_thread_iters = 1000;

    using range = std::pair<std::size_t, std::size_t>;
    using execution_trace = std::vector< std::vector<range> >;

    execution_trace trace(num_threads);
    for (auto& v : trace)
        v.reserve(repeats);

    for (std::size_t repeat = 0; repeat < repeats; ++repeat) {
        big_arena.execute([&] {
            tbb::parallel_for(
                tbb::blocked_range<std::size_t>(0, per_thread_iters * num_threads),
                [&](const tbb::blocked_range<std::size_t>& r) {
                    int thread_id = tbb::this_task_arena::current_thread_index();
                    trace[thread_id].emplace_back(r.begin(), r.end());

                    const bool is_uniform_split = r.size() == per_thread_iters;
                    CHECK_MESSAGE(is_uniform_split, "static partitioner split the range incorrectly.");

                    std::this_thread::yield();

                    std::forward<PerBodyFunc>(body)();
                },
                tbb::static_partitioner()
            );
        });
        // TODO:
        //   - Consider introducing an observer to guarantee the threads left the arena.
    }

    std::size_t range_shifts = 0;
    for (std::size_t thread_id = 0; thread_id < num_threads; ++thread_id) {
        auto trace_size = trace[thread_id].size();
        if (trace_size > 1) {
            auto previous_call_range = trace[thread_id][1];

            for (std::size_t invocation = 2; invocation < trace_size; ++invocation) {
                const auto& current_call_range = trace[thread_id][invocation];

                const bool is_range_changed = previous_call_range != current_call_range;
                if (is_range_changed) {
                    previous_call_range = current_call_range;
                    // count thread changes its execution strategy
                    ++range_shifts;
                }
            }
        }

#if TBB_USE_DEBUG
        WARN_MESSAGE(
            trace_size <= repeats,
            "Thread " << thread_id << " executed extra " << trace_size - repeats
            << " ranges assigned to other threads."
        );
        WARN_MESSAGE(
            trace_size >= repeats,
            "Thread " << thread_id << " executed " << repeats - trace_size
            << " fewer ranges than expected."
        );
#endif
    }

#if TBB_USE_DEBUG
    WARN_MESSAGE(
        range_shifts == 0,
        "Threads change subranges " << range_shifts << " times out of "
        << num_threads * repeats - num_threads << " possible."
    );
#endif

    return float(range_shifts) / float(repeats * num_threads);
}

void relaxed_test() {
    float range_shifts_part = test(/*per body invocation call*/[]{});
    const float require_tolerance = 0.5f;
    // TODO: investigate why switching could happen in more than half of the cases
    WARN_MESSAGE(
        (0 <= range_shifts_part && range_shifts_part <= require_tolerance),
        "Tasks affinitization was not respected in " << range_shifts_part * 100 << "% of the cases."
    );
}

void strict_test() {
    utils::SpinBarrier barrier(2 * utils::get_platform_max_threads());
    const float tolerance = 1e-5f;
    while (test(/*per body invocation call*/[&barrier] { barrier.wait(); }) > tolerance);
}

} // namespace task_affinity_retention

//! Testing affinitized tasks are not stolen
//! \brief \ref error_guessing
TEST_CASE("Threads respect task affinity") {
    task_affinity_retention::relaxed_test();
    task_affinity_retention::strict_test();
}
