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

/*begin_memory_pool_allocator_example*/
#define TBB_PREVIEW_MEMORY_POOL 1
#include "oneapi/tbb/memory_pool.h"
#include <list>

int main() {
    oneapi::tbb::memory_pool<std::allocator<int>> my_pool;

    typedef oneapi::tbb::memory_pool_allocator<int> pool_allocator_t;
    std::list<int, pool_allocator_t> my_list(pool_allocator_t{my_pool});

    my_list.emplace_back(1);
}
/*end_memory_pool_allocator_example*/
