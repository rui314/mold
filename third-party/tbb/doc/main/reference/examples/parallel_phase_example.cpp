/*
    Copyright (c) 2025 Intel Corporation

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

/*begin_parallel_phase_example*/
#define TBB_PREVIEW_PARALLEL_PHASE 1

#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/parallel_sort.h"

#include <vector>

int main() {
    oneapi::tbb::task_arena ta {
        tbb::task_arena::automatic, /*reserved_for_masters=*/1,
        tbb::task_arena::priority::normal,
        tbb::task_arena::leave_policy::fast
    };

    std::vector<int> data(1000);

    {
        oneapi::tbb::task_arena::scoped_parallel_phase phase{ta};
        ta.execute([&data]() {
            oneapi::tbb::parallel_for(std::size_t(0), data.size(), [&data](std::size_t i) {
                data[i] = static_cast<int>(i*i);
            });
        });

        for (std::size_t i = 1; i < data.size(); ++i) {
            data[i] += data[i-1];
        }

        ta.execute([&data]() {
            oneapi::tbb::parallel_sort(data.begin(), data.end());
        });

    }
}
/*end_parallel_phase_example*/
