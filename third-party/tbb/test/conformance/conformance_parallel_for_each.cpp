/*
    Copyright (c) 2005-2023 Intel Corporation

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

#include "common/parallel_for_each_common.h"

//! \file conformance_parallel_for_each.cpp
//! \brief Test for [algorithms.parallel_for_each] specification

//! Test input access iterator support
//! \brief \ref requirement \ref interface
TEST_CASE("Input iterator support") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        for( size_t depth = 0; depth <= depths_nubmer; ++depth ) {
            g_tasks_expected = 0;
            for ( size_t i=0; i < depth; ++i )
                g_tasks_expected += FindNumOfTasks(g_depths[i].value());
            TestIterator_Const<utils::InputIterator<value_t>>(depth);
            TestIterator_Move<utils::InputIterator<value_t>>(depth);
#if __TBB_CPP14_GENERIC_LAMBDAS_PRESENT
            TestGenericLambdasCommon<utils::InputIterator<value_t>>(depth);
#endif
        }
    }
}

//! Test container based overload
//! \brief \ref requirement \ref interface
TEST_CASE("Container based overload - input iterator based container") {
    container_based_overload_test_case<utils::InputIterator, incremental_functor_const>(/*expected_value*/0);
}

const size_t elements = 10000;
const size_t init_sum = 0;
std::atomic<size_t> element_counter;

template<size_t K>
struct set_to {
    void operator()(size_t& x) const {
        x = K;
        ++element_counter;
    }
};

#include "common/range_based_for_support.h"
#include <functional>
#include <deque>

template<typename... Context>
void WorkProducingTest(Context&... context) {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        using namespace range_based_for_support_tests;
        std::deque<size_t> v(elements, 0);

        element_counter = 0;
        oneapi::tbb::parallel_for_each(v.begin(), v.end(), set_to<0>(), context...);
        REQUIRE_MESSAGE((element_counter == v.size() && element_counter == elements),
            "not all elements were set");
        REQUIRE_MESSAGE(range_based_for_accumulate(v, std::plus<size_t>(), init_sum) == init_sum,
            "elements of v not all ones");

        element_counter = 0;
        oneapi::tbb::parallel_for_each(v, set_to<1>(), context...);
        REQUIRE_MESSAGE((element_counter == v.size() && element_counter == elements),
            "not all elements were set");
        REQUIRE_MESSAGE(range_based_for_accumulate(v, std::plus<size_t>(), init_sum) == v.size(),
            "elements of v not all ones");

        element_counter = 0;
        oneapi::tbb::parallel_for_each(oneapi::tbb::blocked_range<std::deque<size_t>::iterator>(v.begin(), v.end()), set_to<0>(), context...);
        REQUIRE_MESSAGE((element_counter == v.size() && element_counter == elements),
            "not all elements were set");
        REQUIRE_MESSAGE(range_based_for_accumulate(v, std::plus<size_t>(), init_sum) == init_sum,
            "elements of v not all zeros");
    }
}

#if __TBB_CPP17_INVOKE_PRESENT

class ForEachInvokeItem {
public:
    ForEachInvokeItem(std::size_t rv, std::vector<std::size_t>& cv) : real_value(rv), change_vector(cv) {}

    void do_action() const { ++change_vector[real_value]; }

    void do_action_and_feed(oneapi::tbb::feeder<ForEachInvokeItem>& feeder) const {
        CHECK_MESSAGE(change_vector.size() % 2 == 0, "incorrect test setup");
        std::size_t shift = change_vector.size() / 2;
        std::cout << "Process " << real_value << std::endl;
        ++change_vector[real_value];
        if (real_value < shift) {
            std::cout << "Add " << real_value + shift << std::endl;
            feeder.add(ForEachInvokeItem(real_value + shift, change_vector));
        }
    }
private:
    std::size_t real_value;
    std::vector<std::size_t>& change_vector;
};

template <template <class T> typename IteratorType>
void test_pfor_each_invoke_basic() {
    const std::size_t items_count = 10;
    std::vector<ForEachInvokeItem> items_to_proceed;
    std::vector<std::size_t> change_vector(2 * items_count, 0);

    for (std::size_t i = 0; i < items_count; ++i) {
        items_to_proceed.emplace_back(i, change_vector);
    }

    using iterator_type = IteratorType<ForEachInvokeItem>;

    // Test without feeder
    oneapi::tbb::parallel_for_each(iterator_type(items_to_proceed.data()),
                                   iterator_type(items_to_proceed.data() + items_count),
                                   &ForEachInvokeItem::do_action);

    for (std::size_t i = 0; i < items_count; ++i) {
        CHECK(change_vector[i] == 1);
        CHECK(change_vector[i + items_count] == 0);
        change_vector[i] = 0; // reset
    }

    // Test with feeder
    oneapi::tbb::parallel_for_each(iterator_type(items_to_proceed.data()),
                                   iterator_type(items_to_proceed.data() + items_count),
                                   &ForEachInvokeItem::do_action_and_feed);

    for (auto item : change_vector) {
        CHECK(item == 1);
    }
}

#endif

//! Test that all elements were produced
//! \brief \ref requirement \ref stress
TEST_CASE("Test that all elements in range were produced through body (without task_group_context)") {
    WorkProducingTest();
}

//! Test that all elements were produced (with task_group_context)
//! \brief \ref requirement \ref interface \ref stress
TEST_CASE("Test that all elements in range were produced through body (with task_group_context)") {
    oneapi::tbb::task_group_context context;
    WorkProducingTest(context);
}

//! Move iterator test for class that supports both move and copy semantics
//! \brief \ref requirement \ref interface
TEST_CASE("Move Semantics Test | Item: MovePreferable") {
    DoTestMoveSemantics<TestMoveSem::MovePreferable>();
}

//!  Move semantic test for move only class
//! \brief \ref requirement \ref interface
TEST_CASE("Move Semantics | Item: MoveOnly") {
    //  parallel_for_each uses is_copy_constructible to support non-copyable types
    DoTestMoveSemantics<TestMoveSem::MoveOnly>();
}

#if __TBB_CPP17_INVOKE_PRESENT
//! Test that parallel_for_each uses std::invoke to run the body
//! \brief \ref requirement
TEST_CASE("parallel_for_each and std::invoke") {
    test_pfor_each_invoke_basic<utils::InputIterator>();
    test_pfor_each_invoke_basic<utils::ForwardIterator>();
    test_pfor_each_invoke_basic<utils::RandomIterator>();
}

#endif
