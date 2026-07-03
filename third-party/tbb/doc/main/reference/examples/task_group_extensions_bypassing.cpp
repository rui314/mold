/*
    Copyright (c) 2025 UXL Foundation Contributors

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

#include <cstdint>
#include <vector>
#include <iostream>

static constexpr std::size_t serial_threshold = 16;

/*begin_task_group_extensions_bypassing_example*/
#define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
#include "oneapi/tbb/task_group.h"

template <typename Iterator, typename Function>
struct for_task {
    tbb::task_handle operator()() const {
        tbb::task_handle next_task;

        auto size = end - begin;
        if (size < serial_threshold) {
            // Execute the work serially
            for (Iterator it = begin; it != end; ++it) {
                f(*it);
            }
        } else {
            // Enough work to split the range
            Iterator middle = begin + size / 2;

            // Submit the right subtask for execution
            tg.run(for_task<Iterator, Function>{middle, end, f, tg});

            // Bypass the left subtask
            next_task = tg.defer(for_task<Iterator, Function>{begin, middle, f, tg});
        }
        return next_task;
    }

    Iterator begin;
    Iterator end;
    Function f;
    tbb::task_group& tg;
}; // struct for_task

template <typename RandomAccessIterator, typename Function>
void par_for_each(RandomAccessIterator begin, RandomAccessIterator end, Function f) {
    tbb::task_group tg;
    // Run the root task
    tg.run_and_wait(for_task<RandomAccessIterator, Function>{begin, end, std::move(f), tg});
}
/*end_task_group_extensions_bypassing_example*/

int main() {
    constexpr std::size_t N = 1000;

    std::size_t array[N];

    par_for_each(array, array + N, [](std::size_t& item) {
        item = 42;
    });

    for (std::size_t i = 0; i < N; ++i) {
        if (array[i] != 42) {
            std::cerr << "Error in " << i << "index" << std::endl;
            return 1;
        }
    }
}
