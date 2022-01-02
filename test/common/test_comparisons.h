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

#ifndef __TBB_test_common_test_comparisons_H
#define __TBB_test_common_test_comparisons_H

#include "test.h"

#ifndef __TBB_TEST_CPP20_COMPARISONS
#define __TBB_TEST_CPP20_COMPARISONS __TBB_CPP20_COMPARISONS_PRESENT
#endif

#if __TBB_TEST_CPP20_COMPARISONS
#include <compare>
#endif

namespace comparisons_testing {

template <bool ExpectEqual, bool ExpectLess, typename T>
void testTwoWayComparisons( const T& lhs, const T& rhs ) {
    REQUIRE_MESSAGE(((lhs < rhs) == ExpectLess),
                    "Incorrect 2-way comparison result for less operation");
    REQUIRE_MESSAGE(((lhs <= rhs) == (ExpectLess || ExpectEqual)),
                    "Incorrect 2-way comparison result for less or equal operation");
    bool ExpectGreater = ExpectEqual ? false : !ExpectLess;
    REQUIRE_MESSAGE(((lhs > rhs) == ExpectGreater),
                    "Incorrect 2-way comparison result for greater operation");
    REQUIRE_MESSAGE(((lhs >= rhs) == (ExpectGreater || ExpectEqual)),
                    "Incorrect 2-way comparison result for greater or equal operation");
}

template <bool ExpectEqual, typename T>
void testEqualityComparisons( const T& lhs, const T& rhs ) {
    REQUIRE_MESSAGE((lhs == rhs) == ExpectEqual,
                    "Incorrect 2-way comparison result for equal operation");
    REQUIRE_MESSAGE((lhs != rhs) == !ExpectEqual,
                    "Incorrect 2-way comparison result for unequal operation");
}

#if __TBB_TEST_CPP20_COMPARISONS
template <bool ExpectEqual, bool ExpectLess, typename T>
void testThreeWayComparisons( const T& lhs, const T& rhs ) {
    auto three_way_result = lhs <=> rhs;
    REQUIRE_MESSAGE((three_way_result < 0) == ExpectLess,
                    "Incorrect 3-way comparison result for less operation");
    REQUIRE_MESSAGE((lhs <=> rhs <= 0) == (ExpectLess || ExpectEqual),
                    "Incorrect 3-way comparison result for less or equal operation");
    bool ExpectGreater = ExpectEqual ? false : !ExpectLess;
    REQUIRE_MESSAGE((lhs <=> rhs > 0) == ExpectGreater,
                    "Incorrect 3-way comparison result for greater operation");
    REQUIRE_MESSAGE((lhs <=> rhs >= 0) == (ExpectGreater || ExpectEqual),
                    "Incorrect 3-way comparison result for greater or equal operation");
    REQUIRE_MESSAGE((lhs <=> rhs == 0) == ExpectEqual,
                    "Incorrect 3-way comparison result for equal operation");
    REQUIRE_MESSAGE((lhs <=> rhs != 0) == !ExpectEqual,
                    "Incorrect 3-way comparison result for unequal operation");
}

#endif // __TBB_TEST_CPP20_COMPARISONS

template <bool ExpectEqual, bool ExpectLess, typename T>
void testEqualityAndLessComparisons( const T& lhs, const T& rhs ) {
    testEqualityComparisons<ExpectEqual>(lhs, rhs);
    testTwoWayComparisons<ExpectEqual, ExpectLess>(lhs, rhs);
#if __TBB_TEST_CPP20_COMPARISONS
    testThreeWayComparisons<ExpectEqual, ExpectLess>(lhs, rhs);
#endif
}

class TwoWayComparable {
public:
    TwoWayComparable() : n(0) {
        reset();
    }

    TwoWayComparable( std::size_t num ) : n(num) {
        reset();
    }

    static void reset() {
        equal_called = false;
        unequal_called = false;
        less_called = false;
        greater_called = false;
        less_or_equal_called = false;
        greater_or_equal_called = false;
    }

    static bool equal_called;
    static bool unequal_called;
    static bool less_called;
    static bool greater_called;
    static bool less_or_equal_called;
    static bool greater_or_equal_called;

    friend bool operator==( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        equal_called = true;
        return lhs.n == rhs.n;
    }

    friend bool operator!=( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        unequal_called = true;
        return lhs.n != rhs.n;
    }

    friend bool operator<( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        less_called = true;
        return lhs.n < rhs.n;
    }

    friend bool operator>( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        greater_called = true;
        return lhs.n > rhs.n;
    }

    friend bool operator<=( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        less_or_equal_called = true;
        return lhs.n <= rhs.n;
    }

    friend bool operator>=( const TwoWayComparable& lhs, const TwoWayComparable& rhs ) {
        greater_or_equal_called = true;
        return lhs.n >= rhs.n;
    }

protected:
    std::size_t n;

    friend struct std::hash<TwoWayComparable>;
}; // struct TwoWayComparable

bool TwoWayComparable::equal_called = false;
bool TwoWayComparable::unequal_called = false;
bool TwoWayComparable::less_called = false;
bool TwoWayComparable::greater_called = false;
bool TwoWayComparable::less_or_equal_called = false;
bool TwoWayComparable::greater_or_equal_called = false;

// This function should be executed after comparing two objects, containing TwoWayComparables
// using one of the comparison operators (<=>, <, >, <=, >=)
void check_two_way_comparison() {
    REQUIRE_MESSAGE(TwoWayComparable::less_called,
                    "operator < was not called during the comparison");
    REQUIRE_MESSAGE(!TwoWayComparable::greater_called,
                    "operator > was called during the comparison");
    REQUIRE_MESSAGE(!TwoWayComparable::less_or_equal_called,
                    "operator <= was called during the comparison");
    REQUIRE_MESSAGE(!TwoWayComparable::greater_or_equal_called,
                    "operator >= was called during the comparison");
    REQUIRE_MESSAGE(!(TwoWayComparable::equal_called),
                    "operator == was called during the comparison");
    REQUIRE_MESSAGE(!(TwoWayComparable::unequal_called),
                    "operator == was called during the comparison");
    TwoWayComparable::reset();
}

// This function should be executed after comparing two objects, containing TwoWayComparables
// using operator == or !=
void check_equality_comparison() {
    REQUIRE_MESSAGE(TwoWayComparable::equal_called,
                    "operator == was not called during the comparison");
    REQUIRE_MESSAGE(!(TwoWayComparable::unequal_called),
                    "operator != was called during the comparison");
    TwoWayComparable::reset();
}

#if __TBB_TEST_CPP20_COMPARISONS
class ThreeWayComparable : public TwoWayComparable {
public:
    ThreeWayComparable() : TwoWayComparable() { reset(); }

    ThreeWayComparable( std::size_t num ) : TwoWayComparable(num) { reset(); }

    static void reset() {
        TwoWayComparable::reset();
        three_way_called = false;
    }

    static bool three_way_called;

    friend auto operator<=>( const ThreeWayComparable& lhs, const ThreeWayComparable& rhs ) {
        three_way_called = true;
        return lhs.n <=> rhs.n;
    }

    friend bool operator==( const ThreeWayComparable&, const ThreeWayComparable& ) = default;
}; // class ThreeWayComparable

bool ThreeWayComparable::three_way_called = false;

// This function should be executed after comparing objects, containing ThreeWayComparables
// using one of the comparison operators (<=>, <, >, <=, >=)
void check_three_way_comparison() {
    REQUIRE_MESSAGE(ThreeWayComparable::three_way_called, "operator <=> was not called during the comparison");
    REQUIRE_MESSAGE(!ThreeWayComparable::less_called, "operator < was called during the comparison");
    REQUIRE_MESSAGE(!ThreeWayComparable::greater_called, "operator > was called during the comparison");
    REQUIRE_MESSAGE(!ThreeWayComparable::less_or_equal_called, "operator <= was called during the comparison");
    REQUIRE_MESSAGE(!ThreeWayComparable::greater_or_equal_called, "operator >= was called during the comparison");
    ThreeWayComparable::reset();
}

// Required for testing synthesized_three_way_comparison
class ThreeWayComparableOnly {
public:
    ThreeWayComparableOnly() : n(0) {}
    ThreeWayComparableOnly( std::size_t num ) : n(num) {}

    friend auto operator<=>( const ThreeWayComparableOnly& lhs, const ThreeWayComparableOnly& rhs ) {
        return lhs.n <=> rhs.n;
    }
    friend bool operator==( const ThreeWayComparableOnly& lhs, const ThreeWayComparableOnly& rhs ) {
        return lhs.n == rhs.n;
    }
private:
    std::size_t n;
}; // class ThreeWayComparableOnly

// Required for testing synthesized_three_way_comparison
class LessComparableOnly {
public:
    LessComparableOnly() : n(0) {}
    LessComparableOnly( std::size_t num ) : n(num) {}

    friend bool operator<( const LessComparableOnly& lhs, const LessComparableOnly& rhs ) {
        return lhs.n < rhs.n;
    }
    friend bool operator==( const LessComparableOnly& lhs, const LessComparableOnly& rhs ) {
        return lhs.n == rhs.n;
    }
private:
    std::size_t n;
}; // class LessComparableOnly

#endif // __TBB_TEST_CPP20_COMPARISONS
} // namespace comparisons_testing

namespace std {

template <>
struct hash<comparisons_testing::TwoWayComparable> {
    std::size_t operator()( const comparisons_testing::TwoWayComparable& val ) const {
        return std::hash<std::size_t>{}(val.n);
    }
};

} // namespace std

#endif // __TBB_test_common_test_comparisons_H
