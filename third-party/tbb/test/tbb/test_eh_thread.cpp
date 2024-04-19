/*
    Copyright (c) 2020-2022 Intel Corporation

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

#include "common/config.h"

#include "tbb/parallel_for.h"
#include "tbb/global_control.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"

#include <atomic>
#include <condition_variable>
#include <thread>
#include <vector>

//! \file test_eh_thread.cpp
//! \brief Test for [internal] functionality

// On Windows there is no real thread number limit beside available memory.
// Therefore, the test for thread limit is unreasonable.
// TODO: enable limitThreads with sanitizer under docker
#if TBB_USE_EXCEPTIONS && !_WIN32 && !__ANDROID__

#include <limits.h>
#include <sys/types.h>
#include <sys/time.h>
#include <sys/resource.h>

void limitThreads(size_t limit)
{
    rlimit rlim;

    int ret = getrlimit(RLIMIT_NPROC, &rlim);
    CHECK_MESSAGE(0 == ret, "getrlimit has returned an error");

    rlim.rlim_cur = (rlim.rlim_max == (rlim_t)RLIM_INFINITY) ? limit : utils::min(limit, rlim.rlim_max);

    ret = setrlimit(RLIMIT_NPROC, &rlim);
    CHECK_MESSAGE(0 == ret, "setrlimit has returned an error");
}

size_t getThreadLimit() {
    rlimit rlim;

    int ret = getrlimit(RLIMIT_NPROC, &rlim);
    CHECK_MESSAGE(0 == ret, "getrlimit has returned an error");
    return rlim.rlim_cur;
}

static void* thread_routine(void*)
{
    return nullptr;
}

class Thread {
    pthread_t mHandle{};
    bool mValid{};
public:
    Thread() {
        mValid = false;
        pthread_attr_t attr;
        // Limit the stack size not to consume all virtual memory on 32 bit platforms.
        std::size_t stacksize = utils::max(128*1024, PTHREAD_STACK_MIN);
        if (pthread_attr_init(&attr) == 0 && pthread_attr_setstacksize(&attr, stacksize) == 0) {
            mValid = pthread_create(&mHandle, &attr, thread_routine, /* arg = */ nullptr) == 0;
        }
    }
    bool isValid() const { return mValid; }
    void join() {
        pthread_join(mHandle, nullptr);
    }
};

//! Test for exception when too many threads
//! \brief \ref resource_usage
TEST_CASE("Too many threads") {
    if (utils::get_platform_max_threads() < 2) {
        // The test expects that the scheduler will try to create at least one thread.
        return;
    }

    // Some systems set really big limit (e.g. >45К) for the number of processes/threads
    limitThreads(1);
    if (getThreadLimit() == 1) {
        for (int attempt = 0; attempt < 5; ++attempt) {
            Thread thread;
            if (thread.isValid()) {
                WARN_MESSAGE(false, "We were able to create a thread. setrlimit seems having no effect");
                thread.join();
                return;
            }
        }
        bool g_exception_caught = false;
        try {
            // Initialize the library to create worker threads
            tbb::parallel_for(0, 2, [](int) {});
        } catch (const std::exception & e) {
            g_exception_caught = true;
            // Do not CHECK to avoid memory allocation (we can be out of memory)
            if (e.what()== nullptr) {
                FAIL("Exception does not have description");
            }
        }
        // Do not CHECK to avoid memory allocation (we can be out of memory)
        if (!g_exception_caught) {
            FAIL("No exception was thrown on library initialization");
        }
    } else {
        WARN_MESSAGE(false, "setrlimit seems having no effect");
    }
}
#endif
