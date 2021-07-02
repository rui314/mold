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

#include "common/test.h"

#if (!__TBB_WIN8UI_SUPPORT && !defined(WINAPI_FAMILY) && !__ANDROID__)

#include "oneapi/tbb/task.h"
#include "oneapi/tbb/task_group.h"
#include <thread>

//! \file conformance_resumable_tasks.cpp
//! \brief Test for [resumable_tasks] specification

thread_local bool mLocal = false;

//! Test asynchronous resume
//! \brief \ref interface \ref requirement
TEST_CASE("Async test") {
    CHECK(!mLocal);
    mLocal = true;
    bool suspend = false, resume = false;
    std::thread t;
    oneapi::tbb::task::suspend([&t, &suspend, &resume](oneapi::tbb::task::suspend_point sp) {
        suspend = true;
        t = std::thread([sp, &resume] {
            resume = true;
            oneapi::tbb::task::resume(sp);
        });
    });
    CHECK(suspend);
    CHECK(resume);
    CHECK_MESSAGE(mLocal, "The same thread is expected");
    mLocal = false;
    t.join();
}

//! Test parallel resume
//! \brief \ref interface \ref requirement
TEST_CASE("Parallel test") {
    CHECK(!mLocal);
    mLocal = true;
    constexpr int N = 100;
    std::atomic<int> suspend{}, resume{};
    oneapi::tbb::task_group tg;
    for (int i = 0; i < N; ++i) {
        tg.run([&tg, &suspend, &resume] {
            oneapi::tbb::task::suspend([&tg, &suspend, &resume](oneapi::tbb::task::suspend_point sp) {
                ++suspend;
                tg.run([sp, &resume] {
                    ++resume;
                    oneapi::tbb::task::resume(sp);
                });
            });
        });
    }
    tg.wait();
    CHECK(suspend == N);
    CHECK(resume == N);
    CHECK_MESSAGE(mLocal, "The same thread is expected");
}
#endif
