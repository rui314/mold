/*
    Copyright (c) 2005-2022 Intel Corporation

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
#define __TBB_TEST_DEFINE_PRIVATE_PUBLIC 1
#include "common/inject_scheduler.h"
#define private public
#define protected public
#include "tbb/concurrent_queue.h"
#undef protected
#undef private

#include <limits>

//! \file test_concurrent_queue_whitebox.cpp
//! \brief Test for [internal] functionality

template <typename Q>
class FloggerBody {
public:
    FloggerBody& operator=( const FloggerBody& ) = delete;

    FloggerBody( Q& queue, std::size_t el_num )
        : q(queue), elem_num(el_num) {}

    void operator()( const int thread_id ) const {
        using value_type = typename Q::value_type;
        value_type elem = value_type(thread_id);
        for (std::size_t i = 0; i < elem_num; ++i) {
            q.push(elem);
            bool res = q.try_pop(elem);
            CHECK_FAST(res);
        }
    }

private:
    Q& q;
    std::size_t elem_num;
}; // class FloggerBody

template <typename Q>
void test_flogger_help( Q& q, std::size_t items_per_page ) {
    std::size_t nq = q.my_queue_representation->n_queue;
    std::size_t reserved_elem_num = nq * items_per_page - 1;
    std::size_t hack_val = std::numeric_limits<std::size_t>::max() & ~reserved_elem_num;

    q.my_queue_representation->head_counter = hack_val;
    q.my_queue_representation->tail_counter = hack_val;

    std::size_t k = q.my_queue_representation->tail_counter & -(std::ptrdiff_t)nq;

    for (std::size_t i = 0; i < nq; ++i) {
        q.my_queue_representation->array[i].head_counter = k;
        q.my_queue_representation->array[i].tail_counter = k;
    }

    // To induce the overflow occurrence
    utils::NativeParallelFor(static_cast<typename Q::value_type>(utils::MaxThread), FloggerBody<Q>(q, reserved_elem_num + 20));

    REQUIRE_MESSAGE(q.empty(), "Failed flogger/empty test");
    REQUIRE_MESSAGE(q.my_queue_representation->head_counter < hack_val, "Failed wraparound test");
}

//! \brief \ref error_guessing
TEST_CASE("Test CQ Wrapparound") {
    for (int i = 0; i < 1000; ++i) {
        tbb::concurrent_queue<int> q;
        test_flogger_help(q, q.my_queue_representation->items_per_page);
    }
}

//! \brief \ref error_guessing
TEST_CASE("Test CBQ Wrapparound") {
    for (int i = 0; i < 1000; ++i) {
        tbb::concurrent_bounded_queue<int> q;
        test_flogger_help(q, q.my_queue_representation->items_per_page);
    }
}
