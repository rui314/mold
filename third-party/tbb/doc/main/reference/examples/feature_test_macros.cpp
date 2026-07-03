/*
    Copyright (c) 2026 UXL Foundation Contributors

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

int parallel_loop1_begin = 0;
int parallel_loop1_end = 100;

int parallel_loop2_begin = 100;
int parallel_loop2_end = 1000;

struct parallel_loop_body {
    void operator()(int) const {}
};

using parallel_loop1_body = parallel_loop_body;
using parallel_loop2_body = parallel_loop_body;

/*begin_feature_test_macros_example*/

#define TBB_PREVIEW_PARALLEL_PHASE 1

#include <oneapi/tbb/version.h>
#include <oneapi/tbb/parallel_for.h>

#if TBB_HAS_PARALLEL_PHASE
#include <oneapi/tbb/task_arena.h>
#endif

int main() {
#if TBB_HAS_PARALLEL_PHASE
    tbb::this_task_arena::start_parallel_phase();
#endif

    tbb::parallel_for(parallel_loop1_begin, parallel_loop1_end,
                      parallel_loop1_body{});

    tbb::parallel_for(parallel_loop2_begin, parallel_loop2_end,
                      parallel_loop2_body{});

#if TBB_HAS_PARALLEL_PHASE
    tbb::this_task_arena::end_parallel_phase(/*with_fast_leave=*/true);
#endif
}

/*end_feature_test_macros_example*/
