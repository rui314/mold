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

#include "common/test.h"
#include "common/utils_concurrency_limit.h"
#include "common/cpu_usertime.h"
#include "common/concepts_common.h"
#include "common/iterator.h"

#include "tbb/parallel_sort.h"
#include "tbb/concurrent_vector.h"
#include "tbb/global_control.h"

#include <math.h>
#include <vector>
#include <functional>
#include <string>
#include <cstring>
#include <cstddef>
#include <iterator>
#include <type_traits>

//! \file test_parallel_sort.cpp
//! \brief Test for [algorithms.parallel_sort]

/** Has tightly controlled interface so that we can verify
    that parallel_sort uses only the required interface. */
class Minimal {
    int val;
public:
    void set_val(int i) { val = i; }
    static bool Less (const Minimal &a, const Minimal &b) {
        return (a.val < b.val);
    }
    static bool AreEqual( const Minimal &a, const Minimal &b) {
       return a.val == b.val;
    }
};

//! Defines a comparison function object for Minimal
class MinimalLessCompare {
public:
    bool operator() (const Minimal &a, const Minimal &b) const {
        return Minimal::Less(a,b);
    }
};

template<typename Value>
bool compare(const Value& lhs, const Value& rhs) {
    return lhs == rhs;
}

bool compare(const Minimal& lhs, const Minimal& rhs) {
    return Minimal::AreEqual(lhs, rhs);
}

template<typename Range>
void validate(Range test_range, Range sorted_range) {
    using value_type = typename std::iterator_traits<decltype(std::begin(test_range))>::value_type;
    REQUIRE(
        std::equal(std::begin(test_range), std::end(test_range), std::begin(sorted_range),
            [](const value_type& tested, const value_type& reference) {
                return compare(tested, reference);
            }
        )
    );
}

//! The default initialization routine.
/*! This routine assumes that you can assign to the elements from a float.
    It assumes that iter and sorted_list have already been allocated. It fills
    them according to the current data set (tracked by a local static variable).
    Returns true if a valid test has been setup, or false if there is no test to
    perform.
*/
template <typename RefType, typename ValueType>
void set(RefType& ref, ValueType new_value) {
    ref = static_cast<RefType>(new_value);
}

template <typename ValueType>
void set(Minimal& minimal_ref, ValueType new_value) {
    minimal_ref.set_val(static_cast<int>(new_value));
}

template <typename KeyType>
void set(std::string& string_ref, KeyType key) {
    string_ref = std::to_string(static_cast<float>(key));
}


template <typename RandomAccessIterator, typename Compare>
bool fill_ranges(RandomAccessIterator test_range_begin, RandomAccessIterator sorted_range_begin,
    std::size_t size, const Compare &compare) {

    static char test_case = 0;
    const char num_cases = 3;

    if (test_case < num_cases) {
        // switch on the current test case, filling the test_list and sorted_list appropriately
        switch(test_case) {
            case 0:
                /* use sin to generate the values */
                for (std::size_t i = 0; i < size; i++) {
                    set(test_range_begin[i], sin(float(i)));
                    set(sorted_range_begin[i], sin(float(i)));
                }
                break;
            case 1:
                /* presorted list */
                for (std::size_t i = 0; i < size; i++) {
                    set(test_range_begin[i], i);
                    set(sorted_range_begin[i], i);
                }
                break;
            case 2:
                /* reverse-sorted list */
                for (std::size_t i = 0; i < size; i++) {
                    set(test_range_begin[i], size - i);
                    set(sorted_range_begin[i], size - i);
                }
                break;
        }

        // pre-sort sorted_range for later validity testing
        std::sort(sorted_range_begin, sorted_range_begin + size, compare);
        test_case++;
        return true;
    }
    test_case = 0;
    return false;
}

//! The default test routine.
/*! Tests all data set sizes from 0 to N, all grainsizes from 0 to G=10, and selects from
    all possible interfaces to parallel_sort depending on whether a scratch space and
    compare have been provided.
*/
template<typename ContainerType, std::size_t Size>
struct parallel_sort_test {
    static void run() {
        std::less<typename ContainerType::value_type> default_comp;
        ContainerType range(Size);
        ContainerType sorted_range(Size);

        while (fill_ranges(std::begin(range), std::begin(sorted_range), Size, default_comp)) {
            tbb::parallel_sort(range);
            validate(range, sorted_range);
        }
    }

    template<typename Comparator>
    static void run(Comparator& comp) {
        ContainerType range(Size);
        ContainerType sorted_range(Size);

        while (fill_ranges(std::begin(range), std::begin(sorted_range), Size, comp)) {
            tbb::parallel_sort(range, comp);
            validate(range, sorted_range);
        }
    }
};

template<typename ContainerType, typename Comparator>
void parallel_sort_test_suite() {
    Comparator comp;
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        parallel_sort_test<ContainerType, /*Size*/0    >::run(comp);
        parallel_sort_test<ContainerType, /*Size*/1    >::run(comp);
        parallel_sort_test<ContainerType, /*Size*/10   >::run(comp);
        parallel_sort_test<ContainerType, /*Size*/9999 >::run(comp);
        parallel_sort_test<ContainerType, /*Size*/50000>::run(comp);
    }
}

template<typename ContainerType>
void parallel_sort_test_suite() {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        parallel_sort_test<ContainerType, /*Size*/0    >::run();
        parallel_sort_test<ContainerType, /*Size*/1    >::run();
        parallel_sort_test<ContainerType, /*Size*/10   >::run();
        parallel_sort_test<ContainerType, /*Size*/9999 >::run();
        parallel_sort_test<ContainerType, /*Size*/50000>::run();
    }
}

#if __TBB_CPP20_CONCEPTS_PRESENT
template <typename RandomAccessIterator>
concept can_call_parallel_sort_with_iterator = requires( RandomAccessIterator it ) {
    tbb::parallel_sort(it, it);
};

template <typename RandomAccessIterator, typename Compare>
concept can_call_parallel_sort_with_iterator_and_compare = requires( RandomAccessIterator it, const Compare& compare ) {
    tbb::parallel_sort(it, it, compare);
};

template <typename CBS>
concept can_call_parallel_sort_with_cbs = requires( CBS& cbs ) {
    tbb::parallel_sort(cbs);
};

template <typename CBS, typename Compare>
concept can_call_parallel_sort_with_cbs_and_compare = requires( CBS& cbs, const Compare& compare ) {
    tbb::parallel_sort(cbs, compare);
};

template <typename T>
using CorrectCompare = test_concepts::compare::Correct<T>;

void test_psort_iterator_constraints() {
    using namespace test_concepts::parallel_sort_value;

    static_assert(can_call_parallel_sort_with_iterator<utils::RandomIterator<int>>);
    static_assert(can_call_parallel_sort_with_iterator<typename std::vector<int>::iterator>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::ForwardIterator<int>>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::InputIterator<int>>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::RandomIterator<NonMovableValue>>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::RandomIterator<NonMoveAssignableValue>>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::RandomIterator<NonComparableValue>>);
    static_assert(!can_call_parallel_sort_with_iterator<test_concepts::ConstantIT<int>>);

    static_assert(can_call_parallel_sort_with_iterator_and_compare<utils::RandomIterator<int>, CorrectCompare<int>>);
    static_assert(can_call_parallel_sort_with_iterator_and_compare<typename std::vector<int>::iterator, CorrectCompare<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::ForwardIterator<int>, CorrectCompare<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::InputIterator<int>, CorrectCompare<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::RandomIterator<NonMovableValue>, CorrectCompare<NonMovableValue>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::RandomIterator<NonMoveAssignableValue>, CorrectCompare<NonMoveAssignableValue>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<test_concepts::ConstantIT<int>, CorrectCompare<const int>>);
}

void test_psort_compare_constraints() {
    using namespace test_concepts::compare;

    using CorrectCBS = test_concepts::container_based_sequence::Correct;
    using CorrectIterator = CorrectCBS::iterator;
    static_assert(can_call_parallel_sort_with_iterator_and_compare<CorrectIterator, Correct<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<CorrectIterator, NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<CorrectIterator, WrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<CorrectIterator, WrongSecondInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<CorrectIterator, WrongReturnOperatorRoundBrackets<int>>);

    static_assert(can_call_parallel_sort_with_cbs_and_compare<CorrectCBS, Correct<int>>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CorrectCBS, NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CorrectCBS, WrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CorrectCBS, WrongSecondInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CorrectCBS, WrongReturnOperatorRoundBrackets<int>>);
}

void test_psort_cbs_constraints() {
    using namespace test_concepts::container_based_sequence;
    using namespace test_concepts::parallel_sort_value;

    static_assert(can_call_parallel_sort_with_cbs<Correct>);
    static_assert(!can_call_parallel_sort_with_cbs<NoBegin>);
    static_assert(!can_call_parallel_sort_with_cbs<NoEnd>);
    static_assert(!can_call_parallel_sort_with_cbs<ForwardIteratorCBS>);
    static_assert(!can_call_parallel_sort_with_cbs<ConstantCBS>);

    static_assert(can_call_parallel_sort_with_cbs<CustomValueCBS<CorrectValue>>);
    static_assert(!can_call_parallel_sort_with_cbs<CustomValueCBS<NonMovableValue>>);
    static_assert(!can_call_parallel_sort_with_cbs<CustomValueCBS<NonMoveAssignableValue>>);
    static_assert(!can_call_parallel_sort_with_cbs<CustomValueCBS<NonComparableValue>>);\

    using CorrectCompare = test_concepts::compare::Correct<int>;
    using NonMovableCompare = test_concepts::compare::Correct<NonMovableValue>;
    using NonMoveAssignableCompare = test_concepts::compare::Correct<NonMoveAssignableValue>;
    static_assert(can_call_parallel_sort_with_cbs_and_compare<Correct, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<NoBegin, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<NoEnd, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<ForwardIteratorCBS, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<ConstantCBS, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CustomValueCBS<NonMovableValue>, NonMovableCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<CustomValueCBS<NonMoveAssignableValue>, NonMoveAssignableCompare>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT

template<typename T>
struct minimal_span {
    minimal_span(T* input_data, std::size_t input_size)
     : data{input_data}
     , size{input_size}
    {}

    T* begin() const {
        return data;
    }
    T* end() const {
        return data + size;
    }
private:
    T* data;
    std::size_t size;
};

//! Minimal array sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("Minimal array sorting test (less comparator)") {
    parallel_sort_test_suite<std::vector<Minimal>, MinimalLessCompare>();
}

//! Float array sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Float array sorting test (default comparator)") {
    parallel_sort_test_suite<std::vector<float>>();
}

//! tbb::concurrent_vector<float> sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_vector<float> sorting test (less comparator)") {
    parallel_sort_test_suite<tbb::concurrent_vector<float>, std::less<float>>();
}

//! tbb::concurrent_vector<float> sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_vector<float> sorting test (default comparator)") {
    parallel_sort_test_suite<tbb::concurrent_vector<float>>();
}

//! Array of strings sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("Array of strings sorting test (less comparator)") {
    parallel_sort_test_suite<std::vector<std::string>, std::less<std::string>>();
}

//! Array of strings sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Array of strings sorting test (default comparator)") {
    parallel_sort_test_suite<std::vector<std::string>>();
}

//! tbb::concurrent_vector<Minimal> sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_vector<Minimal> sorting test (less comparator)") {
    parallel_sort_test_suite<tbb::concurrent_vector<Minimal>, MinimalLessCompare>();
}

constexpr std::size_t array_size = 10000;

template<typename SortFunctor>
void sort_array_test(const SortFunctor& sort_functor) {
    int test_array[array_size];
    for (std::size_t i = 0; i < array_size; ++i)
        test_array[i] = rand() % array_size;

    sort_functor(test_array);

    for (std::size_t i = 0; i < array_size - 1; ++i)
        REQUIRE_MESSAGE(test_array[i] <= test_array[i + 1], "Testing data not sorted");
}

//! Array sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Array sorting test (default comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        sort_array_test([](int (&array)[array_size]) {
            tbb::parallel_sort(array);
        });
    }
}

//! Test array sorting via rvalue span (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Test array sorting via rvalue span (default comparator)") {
    sort_array_test([](int (&array)[array_size]) {
        tbb::parallel_sort(minimal_span<int>{array, array_size});
    });
}

//! Test array sorting via const span (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Test array sorting via const span (default comparator)") {
    sort_array_test([](int (&array)[array_size]) {
        const minimal_span<int> span(array, array_size);
        tbb::parallel_sort(span);
    });
}

//! Test rvalue container with stateful comparator
//! \brief \ref error_guessing
TEST_CASE("Test rvalue container with stateful comparator") {
    // Create sorted range
    std::vector<std::size_t> test_vector(array_size);
    for (std::size_t i = 0; i < array_size; ++i)
        test_vector[i] = i;

    std::atomic<std::size_t> count{0};
    tbb::parallel_sort(std::move(test_vector), [&](std::size_t lhs, std::size_t rhs) {
        ++count;
        return lhs < rhs;
    });

    // The comparator should be called at least (size - 1) times to check that the array is sorted
    REQUIRE_MESSAGE(count >= array_size - 1, "Incorrect comparator calls count");
}

//! Testing workers going to sleep
//! \brief \ref resource_usage
TEST_CASE("That all workers sleep when no work") {
    int test_array[array_size];
    for (std::size_t i = 0; i < array_size; ++i)
        test_array[i] = rand() % array_size;

    tbb::parallel_sort(test_array);
    TestCPUUserTime(utils::get_platform_max_threads());
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("parallel_sort constraints") {
    test_psort_iterator_constraints();
    test_psort_compare_constraints();
    test_psort_cbs_constraints();
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
