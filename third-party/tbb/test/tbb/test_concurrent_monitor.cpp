/*
    Copyright (c) 2020-2021 Intel Corporation

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

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#if _MSC_VER && !defined(__INTEL_COMPILER)
// structure was padded due to alignment specifier
#pragma warning( disable: 4324 )
#endif

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"

#include "tbb/global_control.h"
#include "tbb/task_arena.h"
#include "../../src/tbb/concurrent_monitor.h"
#include "../../src/tbb/misc.cpp"

//! \file test_concurrent_monitor.cpp
//! \brief Test for [internal] functionality

#if TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("Stress test") {
    enum class notification_types {
        notify,
        notify_one,
        notify_all,
        notify_number
    };

    std::size_t threads_number = utils::get_platform_max_threads();

    // Need to prolong lifetime of the exposed concurrent_monitor
    tbb::task_scheduler_handle handler{tbb::attach{}};

    utils::SpinBarrier barrier(threads_number);

    tbb::detail::r1::concurrent_monitor test_monitor;
    {
        tbb::task_arena arena(static_cast<int>(threads_number - 1), 0);


        std::size_t iter_on_operation = 1000;
        std::size_t operation_number = std::size_t(notification_types::notify_number) * iter_on_operation;

        auto thread_func = [&, operation_number] {
            for (std::size_t i = 0; i < operation_number; ++i) {
                tbb::detail::r1::concurrent_monitor::thread_context context{std::uintptr_t(1)};
                test_monitor.prepare_wait(context);
                barrier.wait();
                test_monitor.cancel_wait(context);
            }
        };

        for (std::size_t i = 0; i < threads_number - 1; ++i) {
            arena.enqueue(thread_func);
        }

        for (std::size_t i = 0; i < operation_number; ++i) {
            barrier.wait();
            switch (i / iter_on_operation) {
                case 0:
                {
                    test_monitor.notify([] ( std::uintptr_t ) { return true; });
                    break;
                }
                case 1:
                {
                    test_monitor.notify_one();
                    break;
                }
                case 2:
                {
                    test_monitor.notify_all();
                    break;
                }
            };
        }
    }

    tbb::finalize(handler);
}
#endif // TBB_USE_EXCEPTIONS
