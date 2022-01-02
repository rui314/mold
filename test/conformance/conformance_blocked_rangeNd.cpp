/*
    Copyright (c) 2017-2021 Intel Corporation

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

//! \file conformance_blocked_rangeNd.cpp
//! \brief Test for [preview] functionality

#define TBB_PREVIEW_BLOCKED_RANGE_ND 1
#include "oneapi/tbb/blocked_rangeNd.h"
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
    static_assert((oneapi::tbb::blocked_rangeNd<int, DimAmount>::ndims() == oneapi::tbb::blocked_rangeNd<AbstractValueType, DimAmount>::ndims()),
                         "different amount of dimensions");

    using range_t = oneapi::tbb::blocked_rangeNd<AbstractValueType, DimAmount>;
    using utils_t = range_utils<range_t, DimAmount>;

    // Generate empty range
    range_t r = utils_t::make_range(0, true, &MakeAbstractValue, oneapi::tbb::detail::make_index_sequence<DimAmount>());

    utils::AssertSameType(r.is_divisible(), bool());
    utils::AssertSameType(r.empty(), bool());
    utils::AssertSameType(range_t::ndims(), 0U);

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
    using range_t = oneapi::tbb::blocked_rangeNd<int, DimAmount>;
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

//! Testing blocked_rangeNd construction
//! \brief \ref interface
TEST_CASE("Construction") {
    oneapi::tbb::blocked_rangeNd<int, 1>{ { 0,13,3 } };

    oneapi::tbb::blocked_rangeNd<int, 1>{ oneapi::tbb::blocked_range<int>{ 0,13,3 } };

    oneapi::tbb::blocked_rangeNd<int, 2>(oneapi::tbb::blocked_range<int>(-8923, 8884, 13), oneapi::tbb::blocked_range<int>(-8923, 5, 13));

    oneapi::tbb::blocked_rangeNd<int, 2>({ -8923, 8884, 13 }, { -8923, 8884, 13 });

    oneapi::tbb::blocked_range<int> r1(0, 13);

    oneapi::tbb::blocked_range<int> r2(-12, 23);

    oneapi::tbb::blocked_rangeNd<int, 2>({ { -8923, 8884, 13 }, r1});

    oneapi::tbb::blocked_rangeNd<int, 2>({ r2, r1 });

    oneapi::tbb::blocked_rangeNd<int, 2>(r1, r2);

    oneapi::tbb::blocked_rangeNd<AbstractValueType, 4>({ MakeAbstractValue(-3), MakeAbstractValue(13), 8 },
                                               { MakeAbstractValue(-53), MakeAbstractValue(23), 2 },
                                               { MakeAbstractValue(-23), MakeAbstractValue(33), 1 },
                                               { MakeAbstractValue(-13), MakeAbstractValue(43), 7 });
}

static const std::size_t N = 4;

//! Testing blocked_rangeNd interface
//! \brief \ref interface \ref requirement
TEST_CASE("Serial test") {
    SerialTest<N>();
}

//! Testing blocked_rangeNd interface with parallel_for
//! \brief \ref requirement
TEST_CASE("Parallel test") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);
        ParallelTest<N>();
    }
}

//! Testing blocked_rangeNd with proportional splitting
//! \brief \ref interface \ref requirement
TEST_CASE("blocked_rangeNd proportional splitting") {
    oneapi::tbb::blocked_rangeNd<int, 2> original{{0, 100}, {0, 100}};
    oneapi::tbb::blocked_rangeNd<int, 2> first(original);
    oneapi::tbb::proportional_split ps(3, 1);
    oneapi::tbb::blocked_rangeNd<int, 2> second(first, ps);

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
