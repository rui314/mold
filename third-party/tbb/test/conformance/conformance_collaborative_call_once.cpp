/*
    Copyright (c) 2021 Intel Corporation

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

#if _MSC_VER && !defined(__INTEL_COMPILER)
    // unreachable code
    #pragma warning( push )
    #pragma warning( disable: 4702 )
#endif

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "common/test.h"
#include "oneapi/tbb/collaborative_call_once.h"

#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/task_group.h"

#include <type_traits>
#include <exception>

//! \file conformance_collaborative_call_once.cpp
//! \brief Test for [algorithms.collaborative_call_once] specification

//! Test for collaborative_once_flag member functions to be matched with spec
//! \brief \ref interface \ref requirement
TEST_CASE("collaborative_once_flag member functions match") {
    REQUIRE_MESSAGE(std::is_default_constructible<oneapi::tbb::collaborative_once_flag>::value == true, 
        "collaborative_once_flag must be default constructible");
    REQUIRE_MESSAGE(std::is_copy_constructible<oneapi::tbb::collaborative_once_flag>::value == false, 
        "collaborative_once_flag must not be copy constructible");
    REQUIRE_MESSAGE(std::is_copy_assignable<oneapi::tbb::collaborative_once_flag>::value == false, 
        "collaborative_once_flag must not be copy assignable");
    REQUIRE_MESSAGE(std::is_move_constructible<oneapi::tbb::collaborative_once_flag>::value == false, 
        "collaborative_once_flag must not be move constructible");
    REQUIRE_MESSAGE(std::is_move_assignable<oneapi::tbb::collaborative_once_flag>::value == false, 
        "collaborative_once_flag must not be move assignable");
}

//! Test for collaborative_call_once to execute function exactly once
//! \brief \ref interface \ref requirement
TEST_CASE("collaborative_call_once executes function exactly once") {
    oneapi::tbb::collaborative_once_flag once_flag;

    for (int iter = 0; iter < 100; ++iter) {
        oneapi::tbb::collaborative_call_once(once_flag, [](int number) {
            // Will be executed only on first iteration
            REQUIRE(number == 0);
        }, iter);
    }

    // concurrent call
    std::size_t num_threads = utils::get_platform_max_threads();
    utils::SpinBarrier barrier{num_threads};

    int flag = 0;
    auto func = [&flag] { flag++; };

    oneapi::tbb::collaborative_once_flag once_flag_concurrent;
    utils::NativeParallelFor(num_threads, [&](std::size_t) {
        barrier.wait();
        oneapi::tbb::collaborative_call_once(once_flag_concurrent, func);
    });
    REQUIRE(flag == 1);
}


#if TBB_USE_EXCEPTIONS

//! Exception is received only by winner thread
//! \brief \ref error_guessing \ref requirement
TEST_CASE("Exception is received only by winner thread") {
    int num_threads = static_cast<int>(utils::get_platform_max_threads());
    utils::SpinBarrier barrier(num_threads);

    oneapi::tbb::task_group tg;
    oneapi::tbb::collaborative_once_flag flag;

    for (int i = 0; i < num_threads-1; ++i) {
        tg.run([&flag, &barrier] {
            barrier.wait();
            try {
                oneapi::tbb::collaborative_call_once(flag, [] { });
            } catch(...) {
                REQUIRE_MESSAGE(false, "Unreachable code");
            }
        });
    };

    bool exception_happened{false};
    try {
        oneapi::tbb::collaborative_call_once(flag, [&barrier] {
            barrier.wait();
            throw std::exception{};
        });
    } catch (std::exception&) {
        exception_happened = true;
    }

    REQUIRE_MESSAGE(exception_happened == true, "Exception hasn't been received from the winner thread");
    tg.wait();
}

#endif

#if _MSC_VER && !defined(__INTEL_COMPILER)
    #pragma warning( pop )
#endif
