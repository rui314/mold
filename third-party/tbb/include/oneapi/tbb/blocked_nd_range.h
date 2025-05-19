/*
    Copyright (c) 2017-2025 Intel Corporation

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

#ifndef __TBB_blocked_nd_range_H
#define __TBB_blocked_nd_range_H

#include <algorithm>    // std::any_of
#include <array>
#include <cstddef>
#include <type_traits>  // std::is_same, std::enable_if

#include "detail/_config.h"
#include "detail/_template_helpers.h" // index_sequence, make_index_sequence
#include "detail/_namespace_injection.h"
#include "detail/_range_common.h"

#include "blocked_range.h"

namespace tbb {
namespace detail {
namespace d1 {

/*
    The blocked_nd_range_impl uses make_index_sequence<N> to automatically generate a ctor with
    exactly N arguments of the type tbb::blocked_range<Value>. Such ctor provides an opportunity
    to use braced-init-list parameters to initialize each dimension.
    Use of parameters, whose representation is a braced-init-list, but they're not
    std::initializer_list or a reference to one, produces a non-deduced context
    within template argument deduction.

    NOTE: blocked_nd_range must be exactly a templated alias to the blocked_nd_range_impl
    (and not e.g. a derived class), otherwise it would need to declare its own ctor
    facing the same problem that the impl class solves.
*/

template<typename Value, unsigned int N, typename = detail::make_index_sequence<N>>
class blocked_nd_range_impl;

template<typename Value, unsigned int N, std::size_t... Is>
class blocked_nd_range_impl<Value, N, detail::index_sequence<Is...>> {
public:
    //! Type of a value.
    using value_type = Value;

    //! Type of a dimension range.
    using dim_range_type = tbb::blocked_range<value_type>;

    //! Type for the size of a range.
    using size_type = typename dim_range_type::size_type;

    blocked_nd_range_impl() = delete;

    //! Constructs N-dimensional range over N half-open intervals each represented as tbb::blocked_range<Value>.
    blocked_nd_range_impl(const indexed_t<dim_range_type, Is>&... args) : my_dims{ {args...} } {}

#if __clang__ && __TBB_CLANG_VERSION < 140000
    // On clang prior to version 14.0.0, passing a single braced init list to the constructor of blocked_nd_range<T, 1>
    // matches better on the C array constructor and generates compile-time error because of unexpected size
    // Adding constraints for this constructor to force the compiler to drop it from overload resolution if the size is unexpected
    template <unsigned int M, typename = typename std::enable_if<M == N>::type>
    blocked_nd_range_impl(const value_type (&size)[M], size_type grainsize = 1) :
#else
    blocked_nd_range_impl(const value_type (&size)[N], size_type grainsize = 1) :
#endif
        my_dims { dim_range_type(0, size[Is], grainsize)... } {}

    //! Dimensionality of a range.
    static constexpr unsigned int dim_count() { return N; }

    //! Range in certain dimension.
    const dim_range_type& dim(unsigned int dimension) const {
        __TBB_ASSERT(dimension < N, "out of bound");
        return my_dims[dimension];
    }

    //------------------------------------------------------------------------
    // Methods that implement Range concept
    //------------------------------------------------------------------------

    //! True if at least one dimension is empty.
    bool empty() const {
        return std::any_of(my_dims.begin(), my_dims.end(), [](const dim_range_type& d) {
            return d.empty();
        });
    }

    //! True if at least one dimension is divisible.
    bool is_divisible() const {
        return std::any_of(my_dims.begin(), my_dims.end(), [](const dim_range_type& d) {
            return d.is_divisible();
        });
    }

    blocked_nd_range_impl(blocked_nd_range_impl& r, proportional_split proportion) : my_dims(r.my_dims) {
        do_split(r, proportion);
    }

    blocked_nd_range_impl(blocked_nd_range_impl& r, split proportion) : my_dims(r.my_dims) {
        do_split(r, proportion);
    }

private:
    static_assert(N != 0, "zero dimensional blocked_nd_range can't be constructed");

    //! Ranges in each dimension.
    std::array<dim_range_type, N> my_dims;

    template<typename split_type>
    void do_split(blocked_nd_range_impl& r, split_type proportion) {
        static_assert((std::is_same<split_type, split>::value || std::is_same<split_type, proportional_split>::value),
                      "type of split object is incorrect");
        __TBB_ASSERT(r.is_divisible(), "can't split not divisible range");

        auto my_it = std::max_element(my_dims.begin(), my_dims.end(), [](const dim_range_type& first, const dim_range_type& second) {
            return (first.size() * double(second.grainsize()) < second.size() * double(first.grainsize()));
        });

        auto r_it = r.my_dims.begin() + (my_it - my_dims.begin());

        my_it->my_begin = dim_range_type::do_split(*r_it, proportion);

        // (!(my_it->my_begin < r_it->my_end) && !(r_it->my_end < my_it->my_begin)) equals to
        // (my_it->my_begin == r_it->my_end), but we can't use operator== due to Value concept
        __TBB_ASSERT(!(my_it->my_begin < r_it->my_end) && !(r_it->my_end < my_it->my_begin),
                     "blocked_range has been split incorrectly");
    }
};

template<typename Value, unsigned int N>
         __TBB_requires(blocked_range_value<Value>)
class blocked_nd_range : public blocked_nd_range_impl<Value, N> {
    using base = blocked_nd_range_impl<Value, N>;
    // Making constructors of base class visible
    using base::base;
};

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT && __TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES
// blocked_nd_range(const dim_range_type& dim0, const dim_range_type& dim1, ...)
// while the arguments are passed as braced-init-lists
// Works only for 2 and more arguments since the deduction from
// single braced-init-list or single C-array argument prefers the multi-dimensional range
// Only braced-init-lists of size 2 and 3 are allowed since dim_range_type may only
// be constructed from 2 or 3 arguments
template <typename Value, unsigned int... Ns,
          typename = std::enable_if_t<sizeof...(Ns) >= 2>,
          typename = std::enable_if_t<(... && (Ns == 2 || Ns == 3))>>
blocked_nd_range(const Value (&... dim)[Ns])
-> blocked_nd_range<Value, sizeof...(Ns)>;

// blocked_nd_range(const dim_range_type& dim0, const dim_range_type& dim1, ...)
// while the arguments are passed as blocked_range objects of the same type
template <typename Value, typename... Values,
          typename = std::enable_if_t<(... && std::is_same_v<Value, Values>)>>
blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
-> blocked_nd_range<Value, 1 + sizeof...(Values)>;

// blocked_nd_range(const value_type (&size)[N], size_type grainsize = 1)
template <typename Value, unsigned int N>
blocked_nd_range(const Value (&)[N], typename blocked_nd_range<Value, N>::size_type = 1)
-> blocked_nd_range<Value, N>;

// blocked_nd_range(blocked_nd_range<Value, N>&, oneapi::tbb::split)
template <typename Value, unsigned int N>
blocked_nd_range(blocked_nd_range<Value, N>, oneapi::tbb::split)
-> blocked_nd_range<Value, N>;

// blocked_nd_range(blocked_nd_range<Value, N>&, oneapi::tbb::proportional_split)
template <typename Value, unsigned int N>
blocked_nd_range(blocked_nd_range<Value, N>, oneapi::tbb::proportional_split)
-> blocked_nd_range<Value, N>;

#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT && __TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES

} // namespace d1
} // namespace detail

inline namespace v1 {
using detail::d1::blocked_nd_range;
} // namespace v1
} // namespace tbb

#endif /* __TBB_blocked_nd_range_H */
