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

#include "common/parallel_for_each_common.h"
#include "common/concepts_common.h"
#include <vector>
#include <iterator>

//! \file test_parallel_for_each.cpp
//! \brief Test for [algorithms.parallel_for_each]

//! Test forward access iterator support
//! \brief \ref error_guessing \ref interface
TEST_CASE("Forward iterator support") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for(size_t depth = 0; depth <= depths_nubmer; ++depth) {
            g_tasks_expected = 0;
            for (size_t i=0; i < depth; ++i)
                g_tasks_expected += FindNumOfTasks(g_depths[i].value());
            TestIterator_Modifiable<utils::ForwardIterator<value_t>>(depth);
        }
    }
}

//! Test random access iterator support
//! \brief \ref error_guessing \ref interface
TEST_CASE("Random access iterator support") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for(size_t depth = 0; depth <= depths_nubmer; ++depth) {
            g_tasks_expected = 0;
            for (size_t i=0; i < depth; ++i)
                g_tasks_expected += FindNumOfTasks(g_depths[i].value());
            TestIterator_Modifiable<value_t*>(depth);
        }
    }
}

//! Test const random access iterator support
//! \brief \ref error_guessing \ref interface
TEST_CASE("Const random access iterator support") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for(size_t depth = 0; depth <= depths_nubmer; ++depth) {
            g_tasks_expected = 0;
            for (size_t i=0; i < depth; ++i)
                g_tasks_expected += FindNumOfTasks(g_depths[i].value());
            TestIterator_Const<utils::ConstRandomIterator<value_t>>(depth);
        }
    }

}

//! Test container based overload
//! \brief \ref error_guessing \ref interface
TEST_CASE("Container based overload - forward iterator based container") {
    container_based_overload_test_case<utils::ForwardIterator>(/*expected_value*/1);
}

//! Test container based overload
//! \brief \ref error_guessing \ref interface
TEST_CASE("Container based overload - random access iterator based container") {
    container_based_overload_test_case<utils::RandomIterator>(/*expected_value*/1);
}

// Test for iterators over values convertible to work item type
//! \brief \ref error_guessing \ref interface
TEST_CASE("Using with values convertible to work item type") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        using Iterator = size_t*;
        for(size_t depth = 0; depth <= depths_nubmer; ++depth) {
            g_tasks_expected = 0;
            for (size_t i=0; i < depth; ++i)
                g_tasks_expected += FindNumOfTasks(g_depths[i].value());
            // Test for iterators over values convertible to work item type
            TestIterator_Common<Iterator>(depth);
            TestBody<FakeTaskGeneratorBody_RvalueRefVersion, Iterator>(depth);
            TestBody<TaskGeneratorBody_RvalueRefVersion, Iterator>(depth);
        }
    }
}

//! Testing workers going to sleep
//! \brief \ref resource_usage \ref stress
TEST_CASE("That all workers sleep when no work") {
    const std::size_t N = 100000;
    std::vector<std::size_t> vec(N, 0);

    tbb::parallel_for_each(vec.begin(), vec.end(), [&](std::size_t& in) {
        for (int i = 0; i < 1000; ++i) {
            ++in;
        }
    });
    TestCPUUserTime(utils::get_platform_max_threads());
}

#if __TBB_CPP20_CONCEPTS_PRESENT

template <typename Iterator, typename Body>
concept can_call_parallel_for_each_with_iterator = requires( Iterator it, const Body& body, tbb::task_group_context ctx ) {
    tbb::parallel_for_each(it, it, body);
    tbb::parallel_for_each(it, it, body, ctx);
};

template <typename ContainerBasedSequence, typename Body>
concept can_call_parallel_for_each_with_cbs = requires( ContainerBasedSequence cbs,
                                                        const ContainerBasedSequence const_cbs,
                                                        const Body& body, tbb::task_group_context ctx ) {
    tbb::parallel_for_each(cbs, body);
    tbb::parallel_for_each(cbs, body, ctx);
    tbb::parallel_for_each(const_cbs, body);
    tbb::parallel_for_each(const_cbs, body, ctx);
};

using CorrectCBS = test_concepts::container_based_sequence::Correct;

template <typename Body>
concept can_call_parallel_for_each =
    can_call_parallel_for_each_with_iterator<CorrectCBS::iterator, Body> &&
    can_call_parallel_for_each_with_cbs<CorrectCBS, Body>;

template <typename Iterator>
using CorrectBody = test_concepts::parallel_for_each_body::Correct<decltype(*std::declval<Iterator>())>;

void test_pfor_each_iterator_constraints() {
    using CorrectIterator = typename std::vector<int>::iterator; // random_access_iterator
    using IncorrectIterator = std::ostream_iterator<int>; // output_iterator
    static_assert(can_call_parallel_for_each_with_iterator<CorrectIterator, CorrectBody<CorrectIterator>>);
    static_assert(!can_call_parallel_for_each_with_iterator<IncorrectIterator, CorrectBody<IncorrectIterator>>);
}

void test_pfor_each_container_based_sequence_constraints() {
    using namespace test_concepts::container_based_sequence;
    static_assert(can_call_parallel_for_each_with_cbs<Correct, CorrectBody<Correct::iterator>>);
    static_assert(!can_call_parallel_for_each_with_cbs<NoBegin, CorrectBody<NoBegin::iterator>>);
    static_assert(!can_call_parallel_for_each_with_cbs<NoEnd, CorrectBody<NoEnd::iterator>>);
}

void test_pfor_each_body_constraints() {
    using namespace test_concepts::parallel_for_each_body;
    static_assert(can_call_parallel_for_each<Correct<int>>);
    static_assert(can_call_parallel_for_each<WithFeeder<int>>);
    static_assert(!can_call_parallel_for_each<NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_for_each<WithFeederNoOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_for_each<OperatorRoundBracketsNonConst<int>>);
    static_assert(!can_call_parallel_for_each<WithFeederOperatorRoundBracketsNonConst<int>>);
    static_assert(!can_call_parallel_for_each<WrongInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_for_each<WithFeederWrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_for_each<WithFeederWrongSecondInputOperatorRoundBrackets<int>>);
}

//! \brief \ref error_guessing
TEST_CASE("parallel_for_each constraints") {
    test_pfor_each_iterator_constraints();
    test_pfor_each_container_based_sequence_constraints();
    test_pfor_each_body_constraints();
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT
