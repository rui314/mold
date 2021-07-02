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

#ifndef __TBB_test_common_utils_yield_H
#define __TBB_test_common_utils_yield_H

#include "config.h"
#include <oneapi/tbb/detail/_machine.h>

namespace utils {
#if __TBB_GLIBCXX_THIS_THREAD_YIELD_BROKEN
    static inline void yield() {
        sched_yield();
    }
#else
    using std::this_thread::yield;
#endif
}

#endif // __TBB_test_common_utils_yield_H
