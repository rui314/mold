/*
    Copyright (c) 2005-2025 Intel Corporation

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
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/range_based_for_support.h"
#include "common/config.h"
#include "common/concepts_common.h"

#include "tbb/blocked_range.h"
#include "tbb/blocked_range2d.h"
#include "tbb/blocked_range3d.h"
#include "tbb/blocked_nd_range.h"

//! \file test_blocked_range.cpp
//! \brief Test for [algorithms.blocked_range algorithms.blocked_range2d algorithms.blocked_range3d algorithms.blocked_nd_range] specification

#include <utility> //for std::pair
#include <functional>
#include <vector>

//! Testing blocked_range with range based for
//! \brief \ref interface
TEST_CASE("Range based for") {
    using namespace range_based_for_support_tests;

    const std::size_t sequence_length = 100;
    std::size_t int_array[sequence_length] = {0};

    for (std::size_t i = 0; i < sequence_length; ++i) {
        int_array[i] = i + 1;
    }
    const tbb::blocked_range<std::size_t*> r(int_array, int_array + sequence_length, 1);

    CHECK_MESSAGE(range_based_for_accumulate<std::size_t>(r, std::plus<std::size_t>(), std::size_t(0))
            == gauss_summ_of_int_sequence(sequence_length), "incorrect accumulated value generated via range based for ?");
}

//! Proportional split does not overflow with blocked range
//! \brief \ref error_guessing \ref boundary
TEST_CASE("Proportional split overflow") {
    using tbb::blocked_range;
    using tbb::proportional_split;

    blocked_range<std::size_t> r1(0, std::size_t(-1) / 2);
    std::size_t size = r1.size();
    std::size_t begin = r1.begin();
    std::size_t end = r1.end();

    proportional_split p(1, 3);
    blocked_range<std::size_t> r2(r1, p);

    // overflow-free computation
    std::size_t parts = p.left() + p.right();
    std::size_t int_part = size / parts;
    std::size_t fraction = size - int_part * parts; // fraction < parts
    std::size_t right_idx = int_part * p.right() + fraction * p.right() / parts + 1;
    std::size_t newRangeBegin = end - right_idx;

    // Division in 'right_idx' very likely is inexact also.
    std::size_t tolerance = 1;
    std::size_t diff = (r2.begin() < newRangeBegin) ? (newRangeBegin - r2.begin()) : (r2.begin() - newRangeBegin);
    bool is_split_correct = diff <= tolerance;
    bool test_passed = (r1.begin() == begin && r1.end() == r2.begin() && is_split_correct &&
                        r2.end() == end);
    if (!test_passed) {
        REPORT("Incorrect split of blocked range[%lu, %lu) into r1[%lu, %lu) and r2[%lu, %lu), "
               "must be r1[%lu, %lu) and r2[%lu, %lu)\n", begin, end, r1.begin(), r1.end(), r2.begin(), r2.end(), begin, newRangeBegin, newRangeBegin, end);
        CHECK(test_passed);
    }
}

#if __TBB_CPP20_CONCEPTS_PRESENT

template <bool ExpectSatisfies, typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range, Types> == ExpectSatisfies))
void test_blocked_range_constraint() {}

template <bool ExpectSatisfies, typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range2d, Types, Types> == ExpectSatisfies))
void test_blocked_range2d_constraint() {}

template <typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range2d, Types, test_concepts::Dummy> == false))
void test_blocked_range2d_col_invalid_constraint() {}

template <typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range2d, test_concepts::Dummy, Types> == false))
void test_blocked_range2d_row_invalid_constraint() {}

template <bool ExpectSatisfies, typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range3d, Types, Types, Types> == ExpectSatisfies))
void test_blocked_range3d_constraint() {}

template <typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range3d, test_concepts::Dummy, Types, Types> == false))
void test_blocked_range3d_page_invalid_constraint() {}

template <typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range3d, Types, test_concepts::Dummy, Types> == false))
void test_blocked_range3d_row_invalid_constraint() {}

template <typename... Types>
    requires (... && (utils::well_formed_instantiation<tbb::blocked_range3d, Types, Types, test_concepts::Dummy> == false))
void test_blocked_range3d_col_invalid_constraint() {}

template <typename T>
concept well_formed_blocked_nd_range_instantiation_basic = requires {
    typename tbb::blocked_nd_range<T, 1>;
};

template <typename... Types>
concept well_formed_blocked_nd_range_instantiation = ( ... && well_formed_blocked_nd_range_instantiation_basic<Types> );

//! \brief \ref error_guessing
TEST_CASE("constraints for blocked_range value") {
    using namespace test_concepts::blocked_range_value;
    using const_iterator = typename std::vector<int>::const_iterator;

    test_blocked_range_constraint</*Expected = */true,
                                  Correct, char, int, std::size_t, const_iterator>();

    test_blocked_range_constraint</*Expected = */false,
                                  NonCopyable, NonCopyAssignable, NonDestructible,
                                  NoOperatorLess, OperatorLessNonConst, WrongReturnOperatorLess,
                                  NoOperatorMinus, OperatorMinusNonConst, WrongReturnOperatorMinus,
                                  NoOperatorPlus, OperatorPlusNonConst, WrongReturnOperatorPlus>();
}

//! \brief \ref error_guessing
TEST_CASE("constraints for blocked_range2d value") {
    using namespace test_concepts::blocked_range_value;
    using const_iterator = typename std::vector<int>::const_iterator;

    test_blocked_range2d_constraint</*Expected = */true,
                                    Correct, char, int, std::size_t, const_iterator>();

    test_blocked_range2d_constraint</*Expected = */false,
                                    NonCopyable, NonCopyAssignable, NonDestructible,
                                    NoOperatorLess, OperatorLessNonConst, WrongReturnOperatorLess,
                                    NoOperatorMinus, OperatorMinusNonConst, WrongReturnOperatorMinus,
                                    NoOperatorPlus, OperatorPlusNonConst, WrongReturnOperatorPlus>();

    test_blocked_range2d_row_invalid_constraint<Correct, char, int, std::size_t, const_iterator>();
    test_blocked_range2d_col_invalid_constraint<Correct, char, int, std::size_t, const_iterator>();
}

//! \brief \ref error_guessing
TEST_CASE("constraints for blocked_range3d value") {
    using namespace test_concepts::blocked_range_value;
    using const_iterator = typename std::vector<int>::const_iterator;

    test_blocked_range3d_constraint</*Expected = */true,
                                    Correct, char, int, std::size_t, const_iterator>();

    test_blocked_range3d_constraint</*Expected = */false,
                                    NonCopyable, NonCopyAssignable, NonDestructible,
                                    NoOperatorLess, OperatorLessNonConst, WrongReturnOperatorLess,
                                    NoOperatorMinus, OperatorMinusNonConst, WrongReturnOperatorMinus,
                                    NoOperatorPlus, OperatorPlusNonConst, WrongReturnOperatorPlus>();

    test_blocked_range3d_page_invalid_constraint<Correct, char, int, std::size_t, const_iterator>();
    test_blocked_range3d_row_invalid_constraint<Correct, char, int, std::size_t, const_iterator>();
    test_blocked_range3d_col_invalid_constraint<Correct, char, int, std::size_t, const_iterator>();
}

//! \brief \ref error_guessing
TEST_CASE("constraints for blocked_nd_range value") {
    using namespace test_concepts::blocked_range_value;
    using const_iterator = typename std::vector<int>::const_iterator;

    static_assert(well_formed_blocked_nd_range_instantiation<Correct, char, int, std::size_t, const_iterator>);

    static_assert(!well_formed_blocked_nd_range_instantiation<NonCopyable, NonCopyAssignable, NonDestructible,
                                                              NoOperatorLess, OperatorLessNonConst, WrongReturnOperatorLess,
                                                              NoOperatorMinus, OperatorMinusNonConst, WrongReturnOperatorMinus,
                                                              NoOperatorPlus, OperatorPlusNonConst, WrongReturnOperatorPlus>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT && __TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES
template <typename T>
void test_deduction_guides() {
    using oneapi::tbb::blocked_nd_range;
    static_assert(std::is_constructible<T, int>::value, "Incorrect test setup");
    // T as a grainsize in braced-init-list constructions should be used since only
    // the same type is allowed by the braced-init-list
    static_assert(std::is_convertible<T, typename blocked_nd_range<T, 1>::size_type>::value,
                  "Incorrect test setup");

    std::vector<T> v;
    using iterator = typename decltype(v)::iterator;

    oneapi::tbb::blocked_range<T> dim_range(0, 100);

    blocked_nd_range<T, 2> source_range(dim_range, dim_range);

    {
        blocked_nd_range range(dim_range, dim_range, dim_range);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 3>>);
    }
    {
        blocked_nd_range range({v.begin(), v.end()}, {v.begin(), v.end()});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<iterator, 2>>);
    }
    {
        blocked_nd_range range({T{0}, T{100}}, {T{0}, T{100}, T{5}}, {T{0}, T{100}}, {T{0}, T{100}, T{5}});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 4>>);
    }
    {
        blocked_nd_range range({T{100}});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 1>>);
    }
    {
        T array[1] = {100};
        blocked_nd_range range(array);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 1>>);
    }
    {
        blocked_nd_range range({T{100}}, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 1>>);
    }
    {
        T array[1] = {100};
        blocked_nd_range range(array, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 1>>);
    }
    {
        blocked_nd_range range({T{100}, T{200}}, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 2>>);
    }
    {
        blocked_nd_range range({T{100}, T{200}});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 2>>);
    }
    {
        T array[2] = {100, 200};
        blocked_nd_range range(array, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 2>>);
    }
    {
        blocked_nd_range range({T{100}, T{200}, T{300}}, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 3>>);
    }
    {
        blocked_nd_range range({T{100}, T{200}, T{300}});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 3>>);
    }
    {
        T array[3] = {100, 200, 300};
        blocked_nd_range range(array, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 3>>);
    }
    {
        blocked_nd_range range({T{100}, T{200}, T{300}, T{400}});
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 4>>);
    }
    {
        T array[4] = {100, 200, 300, 400};
        blocked_nd_range range(array);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 4>>);

    }
    {
        blocked_nd_range range({T{100}, T{200}, T{300}, T{400}}, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 4>>);
    }
    {
        T array[4] = {100, 200, 300, 400};
        blocked_nd_range range(array, 5);
        static_assert(std::is_same_v<decltype(range), blocked_nd_range<T, 4>>);
    }
    {
        blocked_nd_range range(source_range, oneapi::tbb::split{});
        static_assert(std::is_same_v<decltype(range), decltype(source_range)>);
    }
    {
        blocked_nd_range range(source_range, oneapi::tbb::proportional_split{1, 3});
        static_assert(std::is_same_v<decltype(range), decltype(source_range)>);
    }
    {
        blocked_nd_range range(source_range);
        static_assert(std::is_same_v<decltype(range), decltype(source_range)>);
    }
    {
        blocked_nd_range range(std::move(source_range));
        static_assert(std::is_same_v<decltype(range), decltype(source_range)>);
    }
}

class fancy_value {
public:
    fancy_value(std::size_t real_value) : my_real_value(real_value) {}
    fancy_value(const fancy_value&) = default;
    ~fancy_value() = default;
    fancy_value& operator=(const fancy_value&) = default;

    friend bool operator<(const fancy_value& lhs, const fancy_value& rhs) {
        return lhs.my_real_value < rhs.my_real_value;
    }
    friend std::size_t operator-(const fancy_value& lhs, const fancy_value& rhs) {
        return lhs.my_real_value - rhs.my_real_value;
    }
    friend std::size_t operator-(const fancy_value& lhs, std::size_t offset) {
        return lhs.my_real_value - offset;
    }
    friend fancy_value operator+(const fancy_value& lhs, std::size_t offset) {
        return fancy_value(lhs.my_real_value + offset);
    }

    operator std::size_t() const {
        return my_real_value;
    }
private:
    std::size_t my_real_value;
};

//! Testing blocked_nd_range deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("blocked_nd_range deduction guides") {
    test_deduction_guides<int>();
    test_deduction_guides<fancy_value>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT && __TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES
