/*
    Copyright (c) 2017-2024 Intel Corporation
    Copyright (c) 2026 UXL Foundation Contributors

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
#include "common/utils_assert.h"
#include "common/utils_concurrency_limit.h"

//! \file conformance_blocked_nd_range.cpp
//! \brief Test for [algorithms.blocked_nd_range] specification

#include "oneapi/tbb/blocked_nd_range.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/global_control.h"

#include <algorithm> // std::for_each
#include <array>

// AbstractValueType class represents Value concept's requirements in the most abstract way
class AbstractValueType {
    int value;
    AbstractValueType() {}
public:
    friend AbstractValueType MakeAbstractValue(int i);
    friend int GetValueOf(const AbstractValueType& v);
};

int GetValueOf(const AbstractValueType& v) { return v.value; }

AbstractValueType MakeAbstractValue(int i) {
    AbstractValueType x;
    x.value = i;
    return x;
}

// operator- returns amount of elements of AbstractValueType between u and v
std::size_t operator-(const AbstractValueType& u, const AbstractValueType& v) {
    return GetValueOf(u) - GetValueOf(v);
}

bool operator<(const AbstractValueType& u, const AbstractValueType& v) {
    return GetValueOf(u) < GetValueOf(v);
}

AbstractValueType operator+(const AbstractValueType& u, std::size_t offset) {
    return MakeAbstractValue(GetValueOf(u) + int(offset));
}

template<typename range_t, unsigned int N>
struct range_utils {
    using val_t = typename range_t::value_type;

    template<typename EntityType, std::size_t DimSize>
    using data_type = std::array<typename range_utils<range_t, N - 1>::template data_type<EntityType, DimSize>, DimSize>;

    template<typename EntityType, std::size_t DimSize>
    static void init_data(data_type<EntityType, DimSize>& data) {
        std::for_each(data.begin(), data.end(), range_utils<range_t, N - 1>::template init_data<EntityType, DimSize>);
    }

    template<typename EntityType, std::size_t DimSize>
    static void increment_data(const range_t& range, data_type<EntityType, DimSize>& data) {
        auto begin = data.begin() + range.dim(N - 1).begin();
        // same as "auto end = out.begin() + range.dim(N - 1).end();"
        auto end = begin + range.dim(N - 1).size();
        for (auto i = begin; i != end; ++i) {
            range_utils<range_t, N - 1>::template increment_data<EntityType, DimSize>(range, *i);
        }
    }

    template<typename EntityType, std::size_t DimSize>
    static void check_data(const range_t& range, data_type<EntityType, DimSize>& data) {
        auto begin = data.begin() + range.dim(N - 1).begin();
        // same as "auto end = out.begin() + range.dim(N - 1).end();"
        auto end = begin + range.dim(N - 1).size();
        for (auto i = begin; i != end; ++i) {
            range_utils<range_t, N - 1>::template check_data<EntityType, DimSize>(range, *i);
        }
    }

// BullseyeCoverage Compile C++ with GCC 5.4 warning suppression
// Sequence points error in braced initializer list
#if __GNUC__ && !defined(__clang__) && !defined(__INTEL_COMPILER)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wsequence-point"
#endif
    template<typename input_t, std::size_t... Is>
    static range_t make_range(std::size_t shift, bool negative, val_t(*gen)(input_t), oneapi::tbb::detail::index_sequence<Is...>) {
        return range_t( { { gen(negative ? -input_t(Is + shift) : 0), gen(input_t(Is + shift)), Is + 1} ... } );
    }
#if __GNUC__ && !defined(__clang__) && !defined(__INTEL_COMPILER)
#pragma GCC diagnostic pop
#endif

    static bool is_empty(const range_t& range) {
        if (range.dim(N - 1).empty()) { return true; }
        return range_utils<range_t, N - 1>::is_empty(range);
    }

    static bool is_divisible(const range_t& range) {
        if (range.dim(N - 1).is_divisible()) { return true; }
        return range_utils<range_t, N - 1>::is_divisible(range);
    }

    static void check_splitting(const range_t& range_split, const range_t& range_new, int(*get)(const val_t&), bool split_checker = false) {
        if (get(range_split.dim(N - 1).begin()) == get(range_new.dim(N - 1).begin())) {
            REQUIRE(get(range_split.dim(N - 1).end()) == get(range_new.dim(N - 1).end()));
        }
        else {
            REQUIRE((get(range_split.dim(N - 1).end()) == get(range_new.dim(N - 1).begin()) && !split_checker));
            split_checker = true;
        }
        range_utils<range_t, N - 1>::check_splitting(range_split, range_new, get, split_checker);
    }

};

template<typename range_t>
struct range_utils<range_t, 0> {
    using val_t = typename range_t::value_type;

    template<typename EntityType, std::size_t DimSize>
    using data_type = EntityType;

    template<typename EntityType, std::size_t DimSize>
    static void init_data(data_type<EntityType, DimSize>& data) { data = 0; }

    template<typename EntityType, std::size_t DimSize>
    static void increment_data(const range_t&, data_type<EntityType, DimSize>& data) { ++data; }

    template<typename EntityType, std::size_t DimSize>
    static void check_data(const range_t&, data_type<EntityType, DimSize>& data) {
        REQUIRE(data == 1);
    }

    static bool is_empty(const range_t&) { return false; }

    static bool is_divisible(const range_t&) { return false; }

    static void check_splitting(const range_t&, const range_t&, int(*)(const val_t&), bool) {}
};

// We need MakeInt function to pass it into make_range as factory function
// because of matching make_range with AbstractValueType and other types too
int MakeInt(int i) { return i; }

template<unsigned int DimAmount>
void SerialTest() {
    static_assert((oneapi::tbb::blocked_nd_range<int, DimAmount>::dim_count() == oneapi::tbb::blocked_nd_range<AbstractValueType, DimAmount>::dim_count()),
                         "different amount of dimensions");

    using range_t = oneapi::tbb::blocked_nd_range<AbstractValueType, DimAmount>;
    using utils_t = range_utils<range_t, DimAmount>;

    // Generate empty range
    range_t r = utils_t::make_range(0, true, &MakeAbstractValue, oneapi::tbb::detail::make_index_sequence<DimAmount>());

    utils::AssertSameType(r.is_divisible(), bool());
    utils::AssertSameType(r.empty(), bool());
    utils::AssertSameType(range_t::dim_count(), 0U);

    REQUIRE((r.empty() == utils_t::is_empty(r) && r.empty()));
    REQUIRE(r.is_divisible() == utils_t::is_divisible(r));

    // Generate not-empty range divisible range
    r = utils_t::make_range(1, true, &MakeAbstractValue, oneapi::tbb::detail::make_index_sequence<DimAmount>());
    REQUIRE((r.empty() == utils_t::is_empty(r) && !r.empty()));
    REQUIRE((r.is_divisible() == utils_t::is_divisible(r) && r.is_divisible()));

    range_t r_new(r, oneapi::tbb::split());
    utils_t::check_splitting(r, r_new, &GetValueOf);

    SerialTest<DimAmount - 1>();
}
template<> void SerialTest<0>() {}

template<unsigned int DimAmount>
void ParallelTest() {
    using range_t = oneapi::tbb::blocked_nd_range<int, DimAmount>;
    using utils_t = range_utils<range_t, DimAmount>;

    // Max size is                                 1 << 20 - 1 bytes
    // Thus size of one dimension's elements is    1 << (20 / DimAmount - 1) bytes
    typename utils_t::template data_type<unsigned char, 1 << (20 / DimAmount - 1)> data;
    utils_t::init_data(data);

    range_t r = utils_t::make_range((1 << (20 / DimAmount - 1)) - DimAmount, false, &MakeInt, oneapi::tbb::detail::make_index_sequence<DimAmount>());

    oneapi::tbb::parallel_for(r, [&data](const range_t& range) {
        utils_t::increment_data(range, data);
    });

    utils_t::check_data(r, data);

    ParallelTest<DimAmount - 1>();
}
template<> void ParallelTest<0>() {}

template <typename T, unsigned int N, typename EqPredicate>
void check_constructed_range(const oneapi::tbb::blocked_nd_range<T, N>& range,
                             const T (&begins)[N], const T (&ends)[N],
                             const std::size_t (&grainsizes)[N],
                             EqPredicate pred)
{
    for (unsigned int dim_index = 0; dim_index < N; ++dim_index) {
        using dim_type = typename oneapi::tbb::blocked_nd_range<T, N>::dim_range_type;
        const dim_type& dim = range.dim(dim_index);
        CHECK_MESSAGE(pred(dim.begin(), begins[dim_index]), "Incorrect begin of the constructed range");
        CHECK_MESSAGE(pred(dim.end(), ends[dim_index]), "Incorrect end of the constructed range");
        CHECK_MESSAGE(dim.grainsize() == grainsizes[dim_index], "Incorrect grainsize of the constructed range");
    }
}

template <typename T, unsigned int N>
void check_constructed_range(const oneapi::tbb::blocked_nd_range<T, N>& range,
                             const T (&begins)[N], const T (&ends)[N],
                             const std::size_t (&grainsizes)[N])
{
    check_constructed_range(range, begins, ends, grainsizes, std::equal_to<T>{});
}

//! Testing blocked_nd_range construction
//! \brief \ref interface
TEST_CASE("Construction") {
    int begin1 = 0;
    int end1 = 13;
    std::size_t grainsize1 = 3;

    int begin2 = -8923;
    int end2 = 8884;
    std::size_t grainsize2 = 13;

    std::size_t default_grainsize = 1;

    {
        oneapi::tbb::blocked_nd_range<int, 1> r{ {begin1, end1, grainsize1} };
        check_constructed_range(r, {begin1}, {end1}, {grainsize1});   
    }
    {
        oneapi::tbb::blocked_nd_range<int, 1> r{ {begin1, end1} };
        check_constructed_range(r, {begin1}, {end1}, {default_grainsize});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 1> r{oneapi::tbb::blocked_range<int>{begin1, end1, grainsize1}};
        check_constructed_range(r, {begin1}, {end1}, {grainsize1});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 1> r{oneapi::tbb::blocked_range<int>{begin1, end1}};
        check_constructed_range(r, {begin1}, {end1}, {default_grainsize});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 2> r{{begin1, end1, grainsize1}, {begin2, end2, grainsize2}};
        check_constructed_range(r, {begin1, begin2}, {end1, end2}, {grainsize1, grainsize2});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 2> r{{begin1, end1}, {begin2, end2, grainsize2}};
        check_constructed_range(r, {begin1, begin2}, {end1, end2}, {default_grainsize, grainsize2});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 2> r{oneapi::tbb::blocked_range<int>{begin1, end1, grainsize1},
                                                oneapi::tbb::blocked_range<int>{begin2, end2, grainsize2}};
        check_constructed_range(r, {begin1, begin2}, {end1, end2}, {grainsize1, grainsize2});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 2> r{oneapi::tbb::blocked_range<int>{begin1, end1},
                                                oneapi::tbb::blocked_range<int>{begin2, end2, grainsize2}};
        check_constructed_range(r, {begin1, begin2}, {end1, end2}, {default_grainsize, grainsize2});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 2> r{{begin1, end1, grainsize1},
                                                oneapi::tbb::blocked_range<int>{begin2, end2, grainsize2}};
        check_constructed_range(r, {begin1, begin2}, {end1, end2}, {grainsize1, grainsize2});
    }

    int sizes[4] = {174, 39, 2481, 93};

    {
        oneapi::tbb::blocked_nd_range<int, 4> r(sizes, grainsize1);
        check_constructed_range(r, {0, 0, 0, 0}, sizes,
                                {grainsize1, grainsize1, grainsize1, grainsize1});
    }
    {
        oneapi::tbb::blocked_nd_range<int, 4> r({sizes[0], sizes[1], sizes[2], sizes[3]}, grainsize2);
        check_constructed_range(r, {0, 0, 0, 0}, sizes,
                                {grainsize2, grainsize2, grainsize2, grainsize2});
    }

    {
        AbstractValueType abstract_begins[4] = {MakeAbstractValue(-3), MakeAbstractValue(-53),
                                                MakeAbstractValue(-23), MakeAbstractValue(-13)};

        AbstractValueType abstract_ends[4] = {MakeAbstractValue(13), MakeAbstractValue(23),
                                              MakeAbstractValue(33), MakeAbstractValue(43)};

        std::size_t grainsizes[4] = {8, 2, 1, 7};

        oneapi::tbb::blocked_nd_range<AbstractValueType, 4> r{{abstract_begins[0], abstract_ends[0], grainsizes[0]},
                                                              {abstract_begins[1], abstract_ends[1], grainsizes[1]},
                                                              {abstract_begins[2], abstract_ends[2], grainsizes[2]},
                                                              {abstract_begins[3], abstract_ends[3], grainsizes[3]}};
        
        auto abstract_eq_pred = [](const AbstractValueType& left, const AbstractValueType& right) {
            return GetValueOf(left) == GetValueOf(right);
        };
        check_constructed_range(r, abstract_begins, abstract_ends, grainsizes, abstract_eq_pred);
    }
}

static const std::size_t N = 4;

//! Testing blocked_nd_range interface
//! \brief \ref interface \ref requirement
TEST_CASE("Serial test") {
    SerialTest<N>();
}

#if !EMSCRIPTEN
//! Testing blocked_nd_range interface with parallel_for
//! \brief \ref requirement
TEST_CASE("Parallel test") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);
        ParallelTest<N>();
    }
}
#endif

//! Testing blocked_nd_range with proportional splitting
//! \brief \ref interface \ref requirement
TEST_CASE("blocked_nd_range proportional splitting") {
    oneapi::tbb::blocked_nd_range<int, 2> original{{0, 100}, {0, 100}};
    oneapi::tbb::blocked_nd_range<int, 2> first(original);
    oneapi::tbb::proportional_split ps(3, 1);
    oneapi::tbb::blocked_nd_range<int, 2> second(first, ps);

    int expected_first_end = static_cast<int>(
        original.dim(0).begin() + ps.left() * (original.dim(0).end() - original.dim(0).begin()) / (ps.left() + ps.right())
    );
    if (first.dim(0).size() == second.dim(0).size()) {
        // Splitting was made by cols
        utils::check_range_bounds_after_splitting(original.dim(1), first.dim(1), second.dim(1), expected_first_end);
    } else {
        // Splitting was made by rows
        utils::check_range_bounds_after_splitting(original.dim(0), first.dim(0), second.dim(0), expected_first_end);
    }
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
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
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
