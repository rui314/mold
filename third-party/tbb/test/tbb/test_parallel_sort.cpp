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
#include <cstdio>

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
    static bool AreEqual( Minimal &a,  Minimal &b) {
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

//! The default validate; but it uses operator== which is not required
template<typename Range>
void validate(Range test_range, Range sorted_range, std::size_t size) {
    for (std::size_t i = 0; i < size; i++) {
        REQUIRE( test_range[i] == sorted_range[i] );
    }
}

//! A validate() specialized to Minimal since it does not define an operator==
void validate(Minimal* test_range, Minimal* sorted_range, std::size_t size) {
    for (std::size_t i = 0; i < size; i++) {
        REQUIRE( Minimal::AreEqual(test_range[i], sorted_range[i]) );
    }
}

//! A validate() specialized to concurrent_vector<Minimal> since it does not define an operator==
void validate(tbb::concurrent_vector<Minimal>::iterator test_range, tbb::concurrent_vector<Minimal>::iterator sorted_range, std::size_t size) {
    for (std::size_t i = 0; i < size; i++) {
        REQUIRE( Minimal::AreEqual(test_range[i], sorted_range[i]) );
    }
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

//! The initialization routine specialized to the class string
/*! strings are created from floats.
*/
bool fill_ranges(std::string* iter, std::string* sorted_list, std::size_t size, const std::less<std::string> &compare) {
    static char test_case = 0;
    const char num_cases = 1;

    if (test_case < num_cases) {
        /* use sin to generate the values */
        for (std::size_t i = 0; i < size; i++) {
            char buffer[20];
// Getting rid of secure warning issued by VC 14 and newer
#if _MSC_VER && __STDC_SECURE_LIB__>=200411
            sprintf_s(buffer, sizeof(buffer), "%f", float(sin(float(i))));
#else
            sprintf(buffer, "%f", float(sin(float(i))));
#endif
            sorted_list[i] = iter[i] = std::string(buffer);
        }

        std::sort(sorted_list, sorted_list + size, compare);
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
template<typename ValueType, std::size_t Size>
struct parallel_sort_test {
    static void run() {
         std::less<ValueType> default_comp;
         ValueType* array = new ValueType[Size];
         ValueType* sorted_array = new ValueType[Size];

         while (fill_ranges(array, sorted_array, Size, default_comp)) {
             tbb::parallel_sort(array, array + Size);
             validate(array, sorted_array, Size);
         }

         delete[] array;
         delete[] sorted_array;
    }

    template<typename Comparator>
    static void run(Comparator& comp) {
         ValueType* array = new ValueType[Size];
         ValueType* sorted_array = new ValueType[Size];

        while (fill_ranges(array, sorted_array, Size, comp)) {
            tbb::parallel_sort(array, array + Size, comp);
            validate(array, sorted_array, Size);
        }

        delete[] array;
        delete[] sorted_array;
    }
};

template<typename ValueType, std::size_t Size>
struct parallel_sort_test<tbb::concurrent_vector<ValueType>, Size> {
    static void run() {
        std::less<ValueType> default_comp;
        tbb::concurrent_vector<ValueType> vector(Size);
        tbb::concurrent_vector<ValueType> sorted_vector(Size);

        while (fill_ranges(std::begin(vector), std::begin(sorted_vector), Size, default_comp)) {
            tbb::parallel_sort(vector);
            validate(std::begin(vector), std::begin(sorted_vector), Size);
        }
    }

    template<typename Comparator>
    static void run(Comparator& comp) {
        tbb::concurrent_vector<ValueType> vector(Size);
        tbb::concurrent_vector<ValueType> sorted_vector(Size);

        while (fill_ranges(std::begin(vector), std::begin(sorted_vector), Size, comp)) {
            tbb::parallel_sort(vector, comp);
            validate(std::begin(vector), std::begin(sorted_vector), Size);
        }
    }
};

template<typename ValueType, typename Comparator>
void parallel_sort_test_suite() {
    Comparator comp;
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        parallel_sort_test<ValueType, /*Size*/0    >::run(comp);
        parallel_sort_test<ValueType, /*Size*/1    >::run(comp);
        parallel_sort_test<ValueType, /*Size*/10   >::run(comp);
        parallel_sort_test<ValueType, /*Size*/9999 >::run(comp);
        parallel_sort_test<ValueType, /*Size*/50000>::run(comp);
    }
}

template<typename ValueType>
void parallel_sort_test_suite() {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        parallel_sort_test<ValueType, /*Size*/0    >::run();
        parallel_sort_test<ValueType, /*Size*/1    >::run();
        parallel_sort_test<ValueType, /*Size*/10   >::run();
        parallel_sort_test<ValueType, /*Size*/9999 >::run();
        parallel_sort_test<ValueType, /*Size*/50000>::run();
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
    static_assert(can_call_parallel_sort_with_iterator<utils::RandomIterator<int>>);
    static_assert(can_call_parallel_sort_with_iterator<typename std::vector<int>::iterator>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::ForwardIterator<int>>);
    static_assert(!can_call_parallel_sort_with_iterator<utils::InputIterator<int>>);

    static_assert(can_call_parallel_sort_with_iterator_and_compare<utils::RandomIterator<int>, CorrectCompare<int>>);
    static_assert(can_call_parallel_sort_with_iterator_and_compare<typename std::vector<int>::iterator, CorrectCompare<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::ForwardIterator<int>, CorrectCompare<int>>);
    static_assert(!can_call_parallel_sort_with_iterator_and_compare<utils::InputIterator<int>, CorrectCompare<int>>);
}

void test_psort_compare_constraints() {
    using namespace test_concepts::compare;
    using CorrectIterator = test_concepts::container_based_sequence::iterator;
    using CorrectCBS = test_concepts::container_based_sequence::Correct;
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
    using CorrectCompare = test_concepts::compare::Correct<int>;
    static_assert(can_call_parallel_sort_with_cbs<Correct>);
    static_assert(!can_call_parallel_sort_with_cbs<NoBegin>);
    static_assert(!can_call_parallel_sort_with_cbs<NoEnd>);
    static_assert(!can_call_parallel_sort_with_cbs<ForwardIteratorCBS>);

    static_assert(can_call_parallel_sort_with_cbs_and_compare<Correct, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<NoBegin, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<NoEnd, CorrectCompare>);
    static_assert(!can_call_parallel_sort_with_cbs_and_compare<ForwardIteratorCBS, CorrectCompare>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT

//! Minimal array sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("Minimal array sorting test (less comparator)") {
    parallel_sort_test_suite<Minimal, MinimalLessCompare>();
}

//! Float array sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Float array sorting test (default comparator)") {
    parallel_sort_test_suite<float>();
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
    parallel_sort_test_suite<std::string, std::less<std::string>>();
}

//! Array of strings sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Array of strings sorting test (default comparator)") {
    parallel_sort_test_suite<std::string>();
}

//! tbb::concurrent_vector<Minimal> sorting test (less comparator)
//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_vector<Minimal> sorting test (less comparator)") {
    parallel_sort_test_suite<tbb::concurrent_vector<Minimal>, MinimalLessCompare>();
}

const int vector_size = 10000;

//! Array sorting test (default comparator)
//! \brief \ref error_guessing
TEST_CASE("Array sorting test (default comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);

        int test_array[vector_size];
        for (int i = 0; i < vector_size; ++i)
            test_array[i] = rand() % vector_size;

        tbb::parallel_sort(test_array);

        for(int i = 0; i < vector_size - 1; ++i)
            REQUIRE_MESSAGE(test_array[i] <= test_array[i + 1], "Testing data not sorted");
    }
}

//! Testing workers going to sleep
//! \brief \ref resource_usage
TEST_CASE("That all workers sleep when no work") {
    int test_array[vector_size];
    for (int i = 0; i < vector_size; ++i)
        test_array[i] = rand() % vector_size;

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
