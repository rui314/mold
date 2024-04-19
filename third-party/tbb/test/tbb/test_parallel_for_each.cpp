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
#include "common/concepts_common.h"
#include <vector>
#include <iterator>

//! \file test_parallel_for_each.cpp
//! \brief Test for [algorithms.parallel_for_each]

#if __TBB_CPP20_PRESENT
// Fancy iterator type that models the C++20 iterator type
// that defines the real iterator category using iterator_concept type
// and iterator_category is always std::input_iterator_type
// Similar iterators are used by C++20 ranges (e.g. std::ranges::iota_view::iterator)
// parallel_for_each algorithm should detect such iterators with respect to iterator_concept value

template <typename T, typename Category>
struct cpp20_iterator {
    static_assert(std::derived_from<Category, std::forward_iterator_tag>,
                  "cpp20_iterator should be of at least forward iterator category");

    using iterator_concept = Category;
    using iterator_category = std::input_iterator_tag;
    using value_type = T;
    using difference_type = std::ptrdiff_t;

    cpp20_iterator() = default;
    explicit cpp20_iterator(T* ptr) : my_ptr(ptr) {}

    T& operator*() const { return *my_ptr; }

    cpp20_iterator& operator++() {
        ++my_ptr;
        return *this;
    }

    cpp20_iterator operator++(int) {
        auto it = *this;
        ++*this;
        return it;
    }

    cpp20_iterator& operator--()
        requires std::derived_from<Category, std::bidirectional_iterator_tag>
    {
        --my_ptr;
        return *this;
    }

    cpp20_iterator operator--(int)
        requires std::derived_from<Category, std::bidirectional_iterator_tag>
    {
        auto it = *this;
        --*this;
        return it;
    }

    cpp20_iterator& operator+=(difference_type n)
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        my_ptr += n;
        return *this;
    }

    cpp20_iterator& operator-=(difference_type n)
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        my_ptr -= n;
        return *this;
    }

    T& operator[](difference_type n) const
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        return my_ptr[n];
    }

    friend bool operator==(const cpp20_iterator&, const cpp20_iterator&) = default;

    friend auto operator<=>(const cpp20_iterator&, const cpp20_iterator&)
        requires std::derived_from<Category, std::random_access_iterator_tag> = default;

    friend cpp20_iterator operator+(cpp20_iterator i, difference_type n)
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        return cpp20_iterator(i.my_ptr + n);
    }

    friend cpp20_iterator operator+(difference_type n, cpp20_iterator i)
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        return i + n;
    }

    friend cpp20_iterator operator-(cpp20_iterator i, difference_type n)
        requires std::derived_from<Category, std::random_access_iterator_tag>
    {
        return cpp20_iterator(i.my_ptr - n);
    }

    friend difference_type operator-(const cpp20_iterator& x, const cpp20_iterator& y) {
        return x.my_ptr - y.my_ptr;
    }
private:
    T* my_ptr = nullptr;
}; // class cpp20_iterator
#endif // __TBB_CPP20_PRESENT

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

#if __TBB_CPP20_PRESENT

struct no_copy_move {
    no_copy_move() = default;

    no_copy_move(const no_copy_move&) = delete;
    no_copy_move(no_copy_move&&) = delete;

    no_copy_move& operator=(const no_copy_move&) = delete;
    no_copy_move& operator=(no_copy_move&&) = delete;

    int item = 0;
};

template <typename Category>
void test_with_cpp20_iterator() {
    constexpr std::size_t n = 1'000'000;

    std::vector<no_copy_move> elements(n);

    cpp20_iterator<no_copy_move, Category> begin(elements.data());
    cpp20_iterator<no_copy_move, Category> end(elements.data() + n);

    oneapi::tbb::parallel_for_each(begin, end, [](no_copy_move& element) {
        element.item = 42;
    });

    for (std::size_t index = 0; index < n; ++index) {
        CHECK(elements[index].item == 42);
    }
}

//! \brief \ref error_guessing \ref regression
TEST_CASE("parallel_for_each with cpp20 iterator") {
    // Test that parallel_for_each threats ignores iterator_category type
    // if iterator_concept type is defined for iterator

    // For input iterators parallel_for_each requires element to be
    // copyable or movable so since cpp20_iterator is at least forward
    // parallel_for_each should work with cpp20_iterator
    // on non-copyable and non-movable type

    // test cpp20_iterator implementation
    using cpp20_forward_iterator = cpp20_iterator<int, std::forward_iterator_tag>;
    using cpp20_bidirectional_iterator = cpp20_iterator<int, std::bidirectional_iterator_tag>;
    using cpp20_random_access_iterator = cpp20_iterator<int, std::random_access_iterator_tag>;

    static_assert(std::forward_iterator<cpp20_forward_iterator>);
    static_assert(!std::bidirectional_iterator<cpp20_forward_iterator>);

    static_assert(std::bidirectional_iterator<cpp20_bidirectional_iterator>);
    static_assert(!std::random_access_iterator<cpp20_bidirectional_iterator>);

    static_assert(std::random_access_iterator<cpp20_random_access_iterator>);

    test_with_cpp20_iterator<std::forward_iterator_tag>();
    test_with_cpp20_iterator<std::bidirectional_iterator_tag>();
    test_with_cpp20_iterator<std::random_access_iterator_tag>();
}

#endif // __TBB_CPP20_PRESENT
