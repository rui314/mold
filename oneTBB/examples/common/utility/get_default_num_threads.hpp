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

#ifndef TBB_examples_num_threads_H
#define TBB_examples_num_threads_H

#include "oneapi/tbb/task_arena.h"

namespace utility {
inline int get_default_num_threads() {
    return oneapi::tbb::this_task_arena::max_concurrency();
}
} // namespace utility

#endif /* TBB_examples_num_threads_H */
