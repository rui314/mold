/*
    Copyright (c) 2020-2023 Intel Corporation

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

#include "tbb/task_group.h"
#include "tbb/task_arena.h"
#include "tbb/global_control.h"

#include "common/spin_barrier.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"

#include <cstddef>
#include <algorithm>
#include <numeric>

//! \file test_arena_priorities.cpp
//! \brief Test for [scheduler.task_arena] specification

//--------------------------------------------------//

std::vector<tbb::task_arena::priority> g_task_info;

std::atomic<unsigned> g_task_num;

std::atomic<bool> g_work_submitted;

namespace HighPriorityArenasTakeExecutionPrecedence {

using concurrency_type = int;
using arena_info =
    std::tuple<std::unique_ptr<tbb::task_arena>,
               concurrency_type,
               tbb::task_arena::priority,
               std::unique_ptr<tbb::task_group>>;

enum arena_info_keys {
    arena_pointer, arena_concurrency, arena_priority, associated_task_group
};

void prepare_logging_data(std::vector<tbb::task_arena::priority>& task_log, unsigned overall_tasks_num) {
    task_log.clear();
    task_log.resize(overall_tasks_num);
    for( auto& record : task_log )
        record = tbb::task_arena::priority::normal;
}

template<typename... ArenaArgs>
tbb::task_arena* do_allocate_and_construct( const ArenaArgs&... arena_args )
{
    const int dummy_max_concurrency = 4;
    const int dummy_reserved_for_masters = 4;

    enum initialization_methods {
        lazy,
        explicit_initialize,
        explicit_initialize_with_different_constructor_parameters,
        initialization_methods_num
    };
    static initialization_methods initialization_method = lazy;

    tbb::task_arena* result_arena = nullptr;

    switch( initialization_method ) {

    case lazy:
        result_arena = new tbb::task_arena( arena_args... );
        break;

    case explicit_initialize:
        result_arena = new tbb::task_arena;
        result_arena->initialize( arena_args... );
        break;

    case explicit_initialize_with_different_constructor_parameters:
    {
        tbb::task_arena tmp(dummy_max_concurrency, dummy_reserved_for_masters);
        result_arena = new tbb::task_arena(tmp);
        result_arena->initialize(arena_args...);
        break;
    }

    default:
        REQUIRE_MESSAGE( false, "Not implemented method of initialization." );
        break;
    }

    int next_value = (initialization_method + 1) % initialization_methods_num;
    initialization_method = (initialization_methods)next_value;

    return result_arena;
}

template<typename FirstArenaArg>
tbb::task_arena* decide_on_arguments(
    const FirstArenaArg& first_arg, const int reserved_for_masters,
    tbb::task_arena::priority a_priority )
{
    const tbb::task_arena::priority default_priority = tbb::task_arena::priority::normal;
    static bool pass_default_priority_implicitly = false;
    if( default_priority == a_priority ) {
        pass_default_priority_implicitly = !pass_default_priority_implicitly;
        if( pass_default_priority_implicitly )
            return do_allocate_and_construct( first_arg, reserved_for_masters );
    }
    return do_allocate_and_construct( first_arg, reserved_for_masters, a_priority );
}


tbb::task_arena* allocate_and_construct_arena(
    int arena_max_concurrency, tbb::task_arena::priority a_priority )
{
    const int reserved_for_masters = 0;

    static bool use_constraints = false;
    use_constraints = !use_constraints;

    if( use_constraints ) {
        tbb::task_arena::constraints properties{tbb::task_arena::automatic, arena_max_concurrency};
        return decide_on_arguments( properties, reserved_for_masters, a_priority );
    }

    return decide_on_arguments( arena_max_concurrency, reserved_for_masters, a_priority );
}

void submit_work( std::vector<arena_info>& arenas, unsigned repeats, utils::SpinBarrier& barrier ) {
    for( auto& item : arenas ) {
        tbb::task_arena& arena = *std::get<arena_pointer>(item).get();
        concurrency_type concurrency = std::get<arena_concurrency>(item);
        tbb::task_arena::priority priority_value = std::get<arena_priority>(item);
        auto& tg = std::get<associated_task_group>(item);

        arena.execute(
            [repeats, &barrier, &tg, priority_value, concurrency]() {
                for( unsigned i = 0; i < repeats * concurrency; ++i ) {
                    tg->run(
                        [&barrier, priority_value](){
                            while( !g_work_submitted.load(std::memory_order_acquire) )
                                utils::yield();
                            g_task_info[g_task_num++] = priority_value;
                            barrier.wait();
                        }
                    );
                }
            } // arena work submission functor
        );
    }
}

void wait_work_completion(
    std::vector<arena_info>& arenas, std::size_t max_num_threads, unsigned overall_tasks_num )
{
    if( max_num_threads > 1 )
        while( g_task_num < overall_tasks_num )
            utils::yield();

    for( auto& item : arenas ) {
        tbb::task_arena& arena = *std::get<arena_pointer>(item).get();
        auto& tg = std::get<associated_task_group>(item);
        arena.execute( [&tg]() { tg->wait(); } );
    }
    CHECK_MESSAGE(g_task_num == overall_tasks_num, "Not all tasks were executed.");
}

void test() {

    const std::size_t max_num_threads = utils::get_platform_max_threads();

    tbb::global_control control(tbb::global_control::max_allowed_parallelism, max_num_threads + 1);
    concurrency_type signed_max_num_threads = static_cast<int>(max_num_threads);
    if (1 == max_num_threads) {
        // Skipping workerless case
        return;
    }

    INFO( "max_num_threads = " << max_num_threads );
    // TODO: iterate over threads to see that the work is going on in low priority arena.

    const int min_arena_concurrency = 2; // implementation detail

    tbb::task_arena::priority high   = tbb::task_arena::priority::high;
    tbb::task_arena::priority normal = tbb::task_arena::priority::normal;
    tbb::task_arena::priority low    = tbb::task_arena::priority::low;

    // TODO: use vector or std::array of priorities instead of the c-style array.

    // TODO: consider extending priorities to have more than three arenas.

    tbb::task_arena::priority priorities[] = {high, normal, low}; // keep it sorted
    const unsigned priorities_num = sizeof(priorities) / sizeof(priorities[0]);
    const unsigned overall_arenas_num = priorities_num;

    std::vector<arena_info> arenas;

    std::vector<unsigned> progressing_arenas( overall_arenas_num, 0 );
    std::iota( progressing_arenas.begin(), progressing_arenas.end(), 1 );

    for( const auto& progressing_arenas_num : progressing_arenas ) {

        INFO( "progressing_arenas_num = " << progressing_arenas_num );

        // TODO: consider populating vector with arenas in separate function.
        unsigned adjusted_progressing_arenas = progressing_arenas_num;

        arenas.clear();
        g_task_num = 0;

        concurrency_type projected_concurrency =
            (signed_max_num_threads + progressing_arenas_num - 1) / progressing_arenas_num;
        projected_concurrency = std::max(min_arena_concurrency, projected_concurrency); // implementation detail
        adjusted_progressing_arenas = signed_max_num_threads / projected_concurrency;

        int threads_left = signed_max_num_threads;

        // Instantiate arenas with necessary concurrency so that progressing arenas consume all
        // available threads.
        for( unsigned arena_idx = 0; arena_idx < overall_arenas_num; ++arena_idx ) {
            tbb::task_arena::priority a_priority = priorities[arena_idx];

            concurrency_type concurrency = projected_concurrency;
            concurrency_type actual_concurrency = projected_concurrency;
            if( threads_left < actual_concurrency ||
                arena_idx == adjusted_progressing_arenas - 1 ) // give all remaining threads to last progressing arena
            {
                concurrency = actual_concurrency = threads_left;
            }

            threads_left -= actual_concurrency;

            if( !actual_concurrency ) {
                concurrency = tbb::task_arena::automatic;
                actual_concurrency = signed_max_num_threads;
            }
            actual_concurrency = std::max( min_arena_concurrency, actual_concurrency ); // implementation detail

            tbb::task_arena* arena = allocate_and_construct_arena(concurrency, a_priority);
            arenas.push_back(
                std::make_tuple(
                    std::unique_ptr<tbb::task_arena>(arena),
                    actual_concurrency,
                    a_priority,
                    std::unique_ptr<tbb::task_group>(new tbb::task_group)
                )
            );
        }

        std::rotate( arenas.begin(), arenas.begin() + progressing_arenas_num - 1, arenas.end() );

        const unsigned repeats = 10;

        unsigned overall_tasks_num = 0;
        for( auto& item : arenas )
            overall_tasks_num += std::get<arena_concurrency>(item) * repeats;

        prepare_logging_data( g_task_info, overall_tasks_num );

        g_work_submitted = false;

        utils::SpinBarrier barrier{ max_num_threads };
        submit_work( arenas, repeats, barrier );

        g_work_submitted = true;

        wait_work_completion( arenas, max_num_threads, overall_tasks_num );

        std::map<tbb::task_arena::priority, unsigned> wasted_tasks;

        tbb::task_arena::priority* end_ptr = priorities + adjusted_progressing_arenas;

        {
            // First epoch - check progressing arenas only
            unsigned overall_progressing_arenas_tasks_num = 0;
            std::map<tbb::task_arena::priority, unsigned> per_priority_tasks_num;

            // Due to indeterministic submission of tasks in the beginning, count tasks priorities up
            // to additional epoch. Assume threads are rebalanced once the work is submitted.
            unsigned last_task_idx = std::min((repeats + 1) * unsigned(max_num_threads), overall_tasks_num);
            for( unsigned i = 0; i < last_task_idx; ++i ) {
                tbb::task_arena::priority p = g_task_info[i];
                ++per_priority_tasks_num[p];

                overall_progressing_arenas_tasks_num += (int)(
                    end_ptr != std::find(priorities, end_ptr, p)
                );

                if( i < max_num_threads || i >= repeats * max_num_threads )
                    ++wasted_tasks[p];
            }

            unsigned expected_overall_progressing_arenas_tasks_num = 0;
            for( unsigned i = 0; i < adjusted_progressing_arenas; ++i ) {
                tbb::task_arena::priority p = priorities[i];
                concurrency_type concurrency = 0;
                for( auto& item : arenas ) {
                    if( std::get<arena_priority>(item) == p ) {
                        concurrency = std::get<arena_concurrency>(item);
                        break;
                    }
                }
                unsigned expected_tasks_num = repeats * concurrency;

                CHECK_MESSAGE( expected_tasks_num == per_priority_tasks_num[p],
                               "Unexpected number of executed tasks in arena with index " << i << " and concurrency = " << concurrency ) ;

                expected_overall_progressing_arenas_tasks_num += expected_tasks_num;
            }
            CHECK_MESSAGE(
                expected_overall_progressing_arenas_tasks_num == overall_progressing_arenas_tasks_num,
                "Number of tasks for progressing arenas mismatched."
            );
        }
        {
            // Other epochs - check remaining arenas
            std::map<tbb::task_arena::priority, unsigned> per_priority_tasks_num;

            std::size_t lower_priority_start = (repeats + 1) * max_num_threads;
            for( std::size_t i = lower_priority_start; i < overall_tasks_num; ++i )
                ++per_priority_tasks_num[ g_task_info[i] ];

            for( auto& e : per_priority_tasks_num ) {
                auto priority = e.first;
                auto tasks_num = e.second;

                auto priorities_it = std::find( end_ptr, priorities + priorities_num, priority );
                CHECK_MESSAGE( priorities_it != priorities + priorities_num,
                               "Tasks from prioritized arena got deferred." );

                auto it = std::find_if(
                    arenas.begin(), arenas.end(),
                    [priority](arena_info& info) {
                        return std::get<arena_priority>(info) == priority;
                    }
                );
                auto per_arena_tasks_num = repeats * std::get<arena_concurrency>(*it);
                CHECK_MESSAGE(
                    tasks_num == per_arena_tasks_num - wasted_tasks[priority],
                    "Incorrect number of tasks from deferred (non-progressing) arenas were executed."
                );
            }
        } // Other epochs
    } // loop over simultaneously progressing arenas

    INFO( "Done\n" );
}

} // namespace HighPriorityArenasTakeExecutionPrecedence


// TODO: nested arena case
//! Test for setting a priority to arena
//! \brief \ref requirement
TEST_CASE("Arena priorities") {
    HighPriorityArenasTakeExecutionPrecedence::test();
}
