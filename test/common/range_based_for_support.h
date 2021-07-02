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

#ifndef __TBB_test_common_range_based_for_support_H
#define __TBB_test_common_range_based_for_support_H

#include "config.h"

#include <utility>

namespace range_based_for_support_tests {

template <typename ValueType, typename Container, typename BinaryAccumulator, typename InitValueType>
inline InitValueType range_based_for_accumulate( const Container& c, BinaryAccumulator accumulator, InitValueType init ) {
    InitValueType range_based_for_accumulated = init;

    for (ValueType x : c) {
        range_based_for_accumulated = accumulator(range_based_for_accumulated, x);
    }
    return range_based_for_accumulated;
}

template <typename Container, typename BinaryAccumulator, typename InitValueType>
inline InitValueType range_based_for_accumulate( const Container& c, BinaryAccumulator accumulator, InitValueType init ) {
    return range_based_for_accumulate<typename Container::value_type>(c, accumulator, init);
}

template <typename IntegralType>
IntegralType gauss_summ_of_int_sequence( IntegralType sequence_length ) {
    return (sequence_length + 1) * sequence_length / 2;
}

struct UnifiedSummer {
    template <typename T>
    T operator()( const T& lhs, const T& rhs ) {
        return lhs + rhs;
    }

    template <typename T, typename U>
    U operator()( const U& lhs, const std::pair<T, U>& rhs ) {
        return lhs + rhs.second;
    }
}; // struct UnifiedSummer

struct pair_second_summer{
    template <typename first_type, typename second_type>
    second_type operator() (second_type const& lhs, std::pair<first_type, second_type> const& rhs) const
    {
        return lhs + rhs.second;
    }
};

} // namespace range_based_for_support_tests

#endif // __TBB_test_common_range_based_for_support_H
