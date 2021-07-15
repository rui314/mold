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

//! \file test_hw_concurrency.cpp
//! \brief Test for [internal] specifications

#include "common/config.h"
#include "common/test.h"
#include "common/utils.h"
#if !__TBB_TEST_SKIP_AFFINITY
#include "common/utils_concurrency_limit.h"
#include "tbb/global_control.h"
#include "tbb/enumerable_thread_specific.h"
#include "tbb/task_arena.h"
#include "tbb/concurrent_vector.h"
#include "tbb/concurrent_queue.h"
#include "tbb/concurrent_priority_queue.h"
#include "tbb/concurrent_hash_map.h"
#include "tbb/concurrent_unordered_map.h"
#include "tbb/concurrent_unordered_set.h"
#include "tbb/concurrent_map.h"
#include "tbb/concurrent_set.h"
#include "tbb/cache_aligned_allocator.h"
#include "tbb/scalable_allocator.h"
#include "tbb/tbb_allocator.h"
#include "tbb/null_mutex.h"
#include "tbb/null_rw_mutex.h"
#include "tbb/queuing_mutex.h"
#include "tbb/queuing_rw_mutex.h"
#include "tbb/spin_mutex.h"
#include "tbb/spin_rw_mutex.h"
#include "tbb/tick_count.h"
#include "tbb/combinable.h"
#include "tbb/blocked_range.h"
#include "tbb/blocked_range2d.h"
#include "tbb/blocked_range3d.h"
#define TBB_PREVIEW_BLOCKED_RANGE_ND 1
#include "tbb/blocked_rangeNd.h"

// Declaration of global objects are needed to check that
// it does not initialize the task scheduler, and in particular
// does not set the default thread number.
// TODO: add other objects that should not initialize the scheduler.
tbb::enumerable_thread_specific<std::size_t> ets;
using vector_ets_type = tbb::enumerable_thread_specific<std::vector<std::size_t>>;
vector_ets_type vets;
tbb::flattened2d<vector_ets_type> f2d(vets);
tbb::combinable<std::size_t> comb;
tbb::concurrent_vector<std::size_t> cv;
tbb::concurrent_queue<std::size_t> cq;
tbb::concurrent_bounded_queue<std::size_t> cbq;
tbb::concurrent_priority_queue<std::size_t> test_cpq;
tbb::concurrent_hash_map<std::size_t, std::size_t> chmap;
tbb::concurrent_unordered_map<std::size_t, std::size_t> cumap;
tbb::concurrent_unordered_multimap<std::size_t, std::size_t> cummap;
tbb::concurrent_unordered_set<std::size_t> cuset;
tbb::concurrent_unordered_multiset<std::size_t> cumset;
tbb::concurrent_map<std::size_t, std::size_t> cmap;
tbb::concurrent_multimap<std::size_t, std::size_t> cmmap;
tbb::concurrent_set<std::size_t> cset;
tbb::concurrent_multiset<std::size_t> cmset;
tbb::cache_aligned_allocator<std::size_t> caa;
tbb::scalable_allocator<std::size_t> sa;
tbb::tbb_allocator<std::size_t> ta;
tbb::null_mutex nm;
tbb::null_rw_mutex nrwm;
tbb::queuing_mutex qm;
tbb::queuing_rw_mutex qrwm;
tbb::spin_mutex sm;
tbb::spin_rw_mutex srwm;
tbb::speculative_spin_mutex ssm;
tbb::speculative_spin_rw_mutex ssrwm;
tbb::tick_count test_tc;
tbb::blocked_range<std::size_t> br(0, 1);
tbb::blocked_range2d<std::size_t> br2d(0, 1, 0, 1);
tbb::blocked_range3d<std::size_t> br3d(0, 1, 0, 1, 0, 1);
tbb::blocked_rangeNd<std::size_t, 2> brNd({0, 1}, {0, 1});

//! \brief \ref error_guessing
TEST_CASE("Check absence of scheduler initialization") {
    int maxProcs = utils::get_max_procs();

    if (maxProcs >= 2) {
        int availableProcs = maxProcs / 2;
        REQUIRE_MESSAGE(utils::limit_number_of_threads(availableProcs) == availableProcs, "limit_number_of_threads has not set the requested limitation");
        REQUIRE(tbb::this_task_arena::max_concurrency() == availableProcs);
    }
}

#endif // !__TBB_TEST_SKIP_AFFINITY
