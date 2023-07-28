/*
    Copyright (c) 2019-2022 Intel Corporation

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

//! \file test_arena_constraints.cpp
//! \brief Test for [info_namespace scheduler.task_arena] specifications

#include "common/common_arena_constraints.h"

#include "tbb/parallel_for.h"

#if __TBB_HWLOC_VALID_ENVIRONMENT
//! Test affinity and default_concurrency correctness for all available constraints.
//! \brief \ref error_guessing
TEST_CASE("Test affinity and default_concurrency correctness for all available constraints.") {
    system_info::initialize();
    for (const auto& constraints: generate_constraints_variety()) {
        tbb::task_arena ta{constraints};
        test_constraints_affinity_and_concurrency(constraints, get_arena_affinity(ta));
    }
}

bool is_observer_created(const tbb::task_arena::constraints& c) {
    std::vector<tbb::core_type_id> core_types = tbb::info::core_types();
    std::vector<tbb::numa_node_id> numa_nodes = tbb::info::numa_nodes();
    return
        (c.numa_id != tbb::task_arena::automatic && numa_nodes.size() > 1) ||
        (c.core_type != tbb::task_arena::automatic && core_types.size() > 1) ||
        c.max_threads_per_core != tbb::task_arena::automatic;
}

void recursive_arena_binding(constraints_container::iterator current_pos, constraints_container::iterator end_pos) {
    system_info::affinity_mask affinity_before = system_info::allocate_current_affinity_mask();

    if (current_pos != end_pos) {
        auto constraints = *current_pos;
        tbb::task_arena current_level_arena{constraints};

        if (is_observer_created(constraints)) {
            system_info::affinity_mask affinity = get_arena_affinity(current_level_arena);
            test_constraints_affinity_and_concurrency(constraints, affinity);
        }

        current_level_arena.execute(
            [&current_pos, &end_pos]() {
                recursive_arena_binding(++current_pos, end_pos);
            }
        );
    }

    system_info::affinity_mask affinity_after = system_info::allocate_current_affinity_mask();
    REQUIRE_MESSAGE(hwloc_bitmap_isequal(affinity_before, affinity_after),
        "After nested arena execution previous affinity mask was not restored.");
}

//! Testing binding correctness during passing through nested arenas
//! \brief \ref interface \ref error_guessing
TEST_CASE("Test binding with nested arenas") {
    system_info::initialize();
    auto constraints_variety = generate_constraints_variety();
    recursive_arena_binding(constraints_variety.begin(), constraints_variety.end());
}


//! Testing constraints propagation during arenas copy construction
//! \brief \ref regression
TEST_CASE("Test constraints propagation during arenas copy construction") {
    system_info::initialize();
    for (const auto& constraints: generate_constraints_variety()) {
        tbb::task_arena constructed{constraints};

        tbb::task_arena copied(constructed);
        system_info::affinity_mask copied_affinity = get_arena_affinity(copied);

        test_constraints_affinity_and_concurrency(constraints, copied_affinity);
    }
}
#endif /*__TBB_HWLOC_VALID_ENVIRONMENT*/

// The test cannot be stabilized with TBB malloc under Thread Sanitizer
#if !__TBB_USE_THREAD_SANITIZER

//! Testing memory leaks absence
//! \brief \ref resource_usage
TEST_CASE("Test memory leaks") {
    constexpr size_t num_trials = 1000;

    // To reduce the test session time only one constraints object is used inside this test.
    // This constraints should use all available settings to cover the most part of tbbbind functionality.
    auto constraints = tbb::task_arena::constraints{}
        .set_numa_id(tbb::info::numa_nodes().front())
        .set_core_type(tbb::info::core_types().front())
        .set_max_threads_per_core(1);

    size_t current_memory_usage = 0, previous_memory_usage = 0, stability_counter = 0;
    bool no_memory_leak = false;
    for (size_t i = 0; i < num_trials; i++) {
        { /* All DTORs must be called before GetMemoryUsage() call*/
            tbb::task_arena arena{constraints};
            arena.execute([]{
                utils::SpinBarrier barrier;
                barrier.initialize(tbb::this_task_arena::max_concurrency());
                tbb::parallel_for(
                    tbb::blocked_range<size_t>(0, tbb::this_task_arena::max_concurrency()),
                    [&barrier](const tbb::blocked_range<size_t>&) {
                        barrier.wait();
                    }
                );
            });
        }

        current_memory_usage = utils::GetMemoryUsage();
        stability_counter = current_memory_usage==previous_memory_usage ? stability_counter + 1 : 0;
        // If the amount of used memory has not changed during 5% of executions,
        // then we can assume that the check was successful
        if (stability_counter > num_trials / 20) {
            no_memory_leak = true;
            break;
        }
        previous_memory_usage = current_memory_usage;
    }
    REQUIRE_MESSAGE(no_memory_leak, "Seems we get memory leak here.");
}
#endif

//! Testing arena constraints setters
//! \brief \ref interface \ref requirement
TEST_CASE("Test arena constraints setters") {
    using constraints = tbb::task_arena::constraints;
    auto constraints_comparison = [](const constraints& c1, const constraints& c2) {
        REQUIRE_MESSAGE(constraints_equal{}(c1, c2),
            "Equal constraints settings specified by different interfaces shows different result.");
    };

    // NUMA node ID setter testing
    for(const auto& numa_index: tbb::info::numa_nodes()) {
        constraints setter_c = constraints{}.set_numa_id(numa_index);
        constraints assignment_c{}; assignment_c.numa_id = numa_index;

        constraints_comparison(setter_c, assignment_c);
    }

    // Core type setter testing
    for(const auto& core_type_index: tbb::info::core_types()) {
        constraints setter_c = constraints{}.set_core_type(core_type_index);
        constraints assignment_c{}; assignment_c.core_type = core_type_index;

        constraints_comparison(setter_c, assignment_c);
    }

    // Max concurrency setter testing
    {
        constraints setter_c = constraints{}.set_max_concurrency(1);
        constraints assignment_c{}; assignment_c.max_concurrency = 1;

        constraints_comparison(setter_c, assignment_c);
    }

    // Threads per core setter testing
    {
        constraints setter_c = constraints{}.set_max_threads_per_core(1);
        constraints assignment_c{}; assignment_c.max_threads_per_core = 1;

        constraints_comparison(setter_c, assignment_c);
    }
}

const int custom_concurrency_value = 42;
void check_concurrency_level(const tbb::task_arena::constraints& c) {
    REQUIRE_MESSAGE(tbb::info::default_concurrency(c) == custom_concurrency_value,
        "Custom arena concurrency was passed to constraints, but was not respected by default_concurrency() call.");
    REQUIRE_MESSAGE(tbb::task_arena{c}.max_concurrency() == custom_concurrency_value,
        "Custom arena concurrency was passed to constraints, but was not respected by default_concurrency() call.");
}

//! Testing concurrency getters output for constraints with custom concurrency value
//! \brief \ref interface \ref error_guessing
TEST_CASE("Test concurrency getters output for constraints with custom concurrency value") {
    tbb::task_arena::constraints c{};
    c.set_max_concurrency(custom_concurrency_value);
    check_concurrency_level(c);

    c.set_numa_id(tbb::info::numa_nodes().front());
    check_concurrency_level(c);

    c.set_core_type(tbb::info::core_types().front());
    check_concurrency_level(c);

    c.set_max_threads_per_core(1);
    check_concurrency_level(c);
}

//! Testing constraints_threads_per_core() reserved entry point
//! \brief \ref error_guessing
TEST_CASE("Testing constraints_threads_per_core() reserved entry point") {
    tbb::task_arena::constraints c{};
    tbb::detail::r1::constraints_threads_per_core(c);
}
