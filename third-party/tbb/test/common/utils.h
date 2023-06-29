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

#ifndef __TBB_test_common_utils_H
#define __TBB_test_common_utils_H

#include "config.h"

#include <oneapi/tbb/detail/_template_helpers.h>
#include <oneapi/tbb/detail/_config.h>
#include <oneapi/tbb/blocked_range.h>
#include <thread>
#include <type_traits>
#include <memory>
#include <array>
#include <cstdint>
#include <vector>
#include <limits>
#include <algorithm>
#include <cstring>
#include <chrono>
#include <unordered_set>

#if HARNESS_TBBMALLOC_THREAD_SHUTDOWN && __TBB_SOURCE_DIRECTLY_INCLUDED && (_WIN32 || _WIN64)
#include "../../src/tbbmalloc/tbbmalloc_internal_api.h"
#endif

#include "dummy_body.h"
#include "utils_yield.h"
#include "utils_assert.h"

namespace utils {

#define utils_fallthrough __TBB_fallthrough

using tbb::detail::try_call;

template<typename It>
typename std::iterator_traits<It>::value_type median(It first, It last) {
    std::sort(first, last);
    typename std::iterator_traits<It>::difference_type distance = std::distance(first, last);
    std::advance(first, distance / 2 - 1);
    if (distance % 2 == 0) {
        // Wrote iterators in variables because of warning: <sequence-point>
        auto curr_element = first;
        auto next_element = ++first;
        return typename std::iterator_traits<It>::value_type((*curr_element + *(++next_element)) / 2);
    } else {
        return typename std::iterator_traits<It>::value_type(*first);
    }
}

static constexpr std::uint8_t MinThread = 1;
static constexpr std::uint8_t MaxThread = 4;

//! Simple native parallel loop where each iteration is executed in a different thread
template <typename Index, typename Body>
void NativeParallelFor( Index Number, const Body& body ) {
    std::vector<std::thread> thread_pool;
    for (Index idx = 0; idx < Number; ++idx) {
        thread_pool.emplace_back([&body, idx] {
            body(idx);
#if HARNESS_TBBMALLOC_THREAD_SHUTDOWN && __TBB_SOURCE_DIRECTLY_INCLUDED && (_WIN32 || _WIN64)
            // in those cases can't release per-thread cache automatically, so do it manually
            // TODO: investigate less-intrusive way to do it, for example via FLS keys
            __TBB_mallocThreadShutdownNotification();
#endif
        });
    }

    for (auto& thread : thread_pool) {
        thread.join();
    }
}

//! Native parallel loop with grainsize (like tbb::blocked_range)
template <typename Index, typename Body>
void NativeParallelFor( Index Number, Index block_size, const Body& body ) {
    NativeParallelFor(Number / block_size, [block_size, &body] (Index idx) {
        for (Index i = idx * block_size; i < (idx + 1) * block_size; ++i) {
            body(i);
        }
    });
}

//! Utility template function to prevent "unused" warnings by various compilers.
template<typename... T> void suppress_unused_warning(T&&...) {}

namespace detail {

    template <std::size_t size>
    struct fixed_width_uint;

    template <> struct fixed_width_uint<1>{ using type = uint8_t;  };
    template <> struct fixed_width_uint<2>{ using type = uint16_t; };
    template <> struct fixed_width_uint<4>{ using type = uint32_t; };
    template <> struct fixed_width_uint<8>{ using type = uint64_t; };

    template <typename In>
    typename fixed_width_uint<sizeof(In)>::type fixed_width_cast(In in) {
        return static_cast<typename fixed_width_uint<sizeof(In)>::type>(in);
    }


    static constexpr std::array<uint64_t, 64> Primes = {{
        0x9e3779b13346320e, 0xffe6cc5974101cb7, 0x2109f6dd6aaac9c9, 0x43977ab5f3dbca42,
        0xba5703f59405b746, 0xb495a877a86fb54e, 0xe1626741ae21caf5, 0x79695e6bc8febd31,
        0xbc98c09f76a304e0, 0xd5bee2b3513a491d, 0x287488f9933e6cb9, 0x3af18231269a8b29,
        0x9677cd4ddbc9d5b1, 0xbe3a6929ddd2a556, 0xadc6a877a2f30f00, 0xdcf0674bb6968d97,
        0xbe4d6fe991c0538d, 0x5f15e201c9cc571e, 0x99afc3fd0f27f767, 0xf3f16801361d4489,
        0xe222cfffee1eec74, 0x24ba5fdb21098d07, 0x0620452d45401c7f, 0x79f149e30a92241f,
        0xc8b93f49e4fe3077, 0x972702cd3aac3d56, 0xb07dd827a9126d73, 0x6c97d5ed60811c65,
        0x085a3d61d2e858f8, 0x46eb5ea7ce433ba1, 0x3d9910edfc8bb30a, 0x2e687b5b6226023c,
        0x296092277d3fd038, 0x6eb081f199767dbe, 0x0954c4e114d147dd, 0x9d114db92a2a629a,
        0x542acfa9232adfb9, 0xb3e6bd7bddd0e31e, 0x0742d917c18e24dc, 0xe9f3ffa78ba59fab,
        0x54581edb3717eaf7, 0xf2480f45494a28c9, 0x0bb9288ff4884f1b, 0xef1affc7bb0a5916,
        0x85fa0ca7da978b79, 0x3ccc14db2137131b, 0xe6baf34b9bb9ade8, 0x343377f7e00c0852,
        0x5ca190311bef1612, 0xe6d9293bc4c93e07, 0xf0a9f391680e1894, 0x5d2e980bb090bd62,
        0xfc41107323c82d43, 0xc3749363812d28e8, 0xb892d829b0357953, 0x3549366b9e23bb94,
        0x629750ad007fd05c, 0xb98294e53416fada, 0x892d9483bb3deae3, 0xc235baf386c925e4,
        0x3d2402a37346a4b0, 0x6bdef3c95be05f43, 0xbec333cd1928a169, 0x40c9520f59e003fa
    }};
}

//! A fast random number generator.
/* Uses linear congruential method. */
template <std::size_t size = 2>
class FastRandom {
public:
    using result_type = typename ::utils::detail::fixed_width_uint<size>::type;

    //! Construct a random number generator.
    explicit FastRandom( uint64_t seed )
      : my_seed(seed),
        my_prime(detail::Primes[my_seed % detail::Primes.size()]) {}

    static constexpr result_type max() {
        return my_max;
    }

    static constexpr result_type min() {
        return my_min;
    }
    //! Get a random number for the given seed; update the seed for next use.
    result_type get( uint64_t seed ) {
        result_type random_number = static_cast<result_type>(my_seed >> (64 - size * CHAR_BIT));
        my_seed = seed * my_prime + 1;
        return random_number;
    }

    //! Get a random number
    result_type get( ) { return get(my_seed); }

    result_type operator() () {
        return static_cast<result_type>(my_seed >> (64 - size * CHAR_BIT)) % my_max;
    }

private:
    uint64_t my_seed, my_prime;
    static constexpr result_type my_max = std::numeric_limits<result_type>::max();
    static constexpr result_type my_min = std::numeric_limits<result_type>::min();
};

namespace iterator_type_traits {

template <typename T> using iterator_traits_value_type = typename std::iterator_traits<T>::value_type;
template <typename T> using iterator_traits_difference_type = typename std::iterator_traits<T>::difference_type;
template <typename T> using iterator_traits_pointer = typename std::iterator_traits<T>::pointer;
template <typename T> using iterator_traits_iterator_category = typename std::iterator_traits<T>::iterator_category;

using std::swap;
template <typename T> using is_swappable = decltype(swap(std::declval<T&>(), std::declval<T&>()));

template <typename T> using is_dereferenceable_by_asterisk = decltype(*std::declval<T>());
template <typename T> using is_dereferenceable_by_arrow = decltype(std::declval<T>().operator->());
template <typename T> using is_preincrementable = decltype(++std::declval<T>());
template <typename T> using is_postincrementable = decltype(std::declval<T>()++);

template <typename T> using is_equality_comparable = decltype(std::declval<T>() == std::declval<T>());
template <typename T> using is_unequality_comparable = decltype(std::declval<T>() != std::declval<T>());

template <typename T> using is_predecrementable = decltype(--std::declval<T>());
template <typename T> using is_postdecrementable = decltype(std::declval<T>()--);

template <typename T> using is_add_assignable = decltype(std::declval<T>() += std::declval<typename T::difference_type>());
template <typename T> using is_sub_assignable = decltype(std::declval<T>() -= std::declval<typename T::difference_type>());

template <typename T> using have_operator_plus = decltype(std::declval<T>() + std::declval<typename T::difference_type>());
template <typename T> using have_operator_minus = decltype(std::declval<T>() + std::declval<typename T::difference_type>());

template <typename T> using have_operator_access = decltype(std::declval<T>()[std::declval<typename T::difference_type>()]);

template <typename T> using have_operator_less = decltype(std::declval<T>() < std::declval<T>());
template <typename T> using have_operator_great = decltype(std::declval<T>() > std::declval<T>());
template <typename T> using have_operator_not_less = decltype(std::declval<T>() <= std::declval<T>());
template <typename T> using have_operator_not_great = decltype(std::declval<T>() >= std::declval<T>());

template <typename T>
using supports_iterator = tbb::detail::supports<T, iterator_traits_value_type,
                                                   iterator_traits_difference_type,
                                                   iterator_traits_pointer,
                                                   iterator_traits_iterator_category,
                                                   is_swappable,
                                                   is_dereferenceable_by_asterisk,
                                                   is_preincrementable>;

template <typename T>
using supports_input_iterator = tbb::detail::supports<T, is_equality_comparable,
                                                         is_unequality_comparable,
                                                         is_dereferenceable_by_arrow,
                                                         is_postincrementable>;
template <typename T>
using supports_bidirectional_iterator = tbb::detail::supports<T, is_predecrementable,
                                                                 is_postdecrementable>;

template <typename T>
using supports_random_access_iterator = tbb::detail::supports<T, is_add_assignable,
                                                                 is_sub_assignable,
                                                                 have_operator_plus,
                                                                 have_operator_minus,
                                                                 have_operator_access,
                                                                 have_operator_less,
                                                                 have_operator_great,
                                                                 have_operator_not_less,
                                                                 have_operator_not_great>;
} // namespace iterator_type_traits

template <typename T>
struct is_iterator : std::integral_constant<bool,
                                            std::is_copy_constructible<T>::value &&
                                            std::is_copy_assignable<T>::value &&
                                            std::is_destructible<T>::value &&
                                            iterator_type_traits::supports_iterator<T>::value> {};

template <typename T>
struct is_input_iterator : std::integral_constant<bool,
                                                  is_iterator<T>::value &&
                                                  iterator_type_traits::supports_input_iterator<T>::value> {};

template <typename T>
struct is_forward_iterator : std::integral_constant<bool,
                                                    is_input_iterator<T>::value &&
                                                    std::is_default_constructible<T>::value> {};

template <typename T>
struct is_bidirectional_iterator : std::integral_constant<bool,
                                                    is_forward_iterator<T>::value &&
                                                    iterator_type_traits::supports_bidirectional_iterator<T>::value> {};

template <typename T>
struct is_random_access_iterator : std::integral_constant<bool,
                                                    is_bidirectional_iterator<T>::value &&
                                                    iterator_type_traits::supports_random_access_iterator<T>::value> {};
// The function to zero-initialize arrays; useful to avoid warnings
template <typename T>
void zero_fill( void* array, std::size_t n ) {
    std::memset(array, 0, sizeof(T) * n);
}

//! Base class that asserts that no operations are made with the object after its destruction.
class NoAfterlife {
protected:
    enum state_t {
        LIVE = 0x56781234,
        DEAD = 0xDEADBEEF
    } m_state;

public:
    NoAfterlife() : m_state(LIVE) {}
    NoAfterlife(const NoAfterlife& src) : m_state(LIVE) {
        CHECK_FAST_MESSAGE(src.IsLive(), "Constructing from the dead source");
    }
    ~NoAfterlife() {
        CHECK_FAST_MESSAGE(IsLive(), "Repeated destructor call");
        m_state = DEAD;
    }
    const NoAfterlife& operator=(const NoAfterlife& src) {
        CHECK_FAST(IsLive());
        CHECK_FAST(src.IsLive());
        return *this;
    }
    void AssertLive() const {
        CHECK_FAST_MESSAGE(IsLive(), "Already dead");
    }
    bool IsLive() const {
        return m_state == LIVE;
    }
}; // NoAfterlife

//! Base class for types that should not be assigned.
class NoAssign {
public:
    NoAssign& operator=(const NoAssign&) = delete;
    NoAssign(const NoAssign&) = default;
    NoAssign() = default;
};

//! Base class for types that should not be copied or assigned.
class NoCopy : NoAssign {
public:
    NoCopy(const NoCopy&) = delete;
    NoCopy() = default;
};

//! Base class for objects which support move ctors
class Movable {
public:
    Movable() : alive(true) {}
    void Reset() { alive = true; }
    Movable(Movable&& other) {
        CHECK_MESSAGE(other.alive, "Moving from a dead object");
        alive = true;
        other.alive = false;
    }
    Movable& operator=(Movable&& other) {
        CHECK_MESSAGE(alive, "Assignment to a dead object");
        CHECK_MESSAGE(other.alive, "Assignment of a dead object");
        other.alive = false;
        return *this;
    }
    Movable& operator=(const Movable& other) {
        CHECK_MESSAGE(alive, "Assignment to a dead object");
        CHECK_MESSAGE(other.alive, "Assignment of a dead object");
        return *this;
    }
    Movable(const Movable& other) {
        CHECK_MESSAGE(other.alive, "Const reference to a dead object");
        alive = true;
    }
    ~Movable() { alive = false; }
    volatile bool alive;
};

class MoveOnly : Movable, NoCopy {
public:
    MoveOnly() : Movable() {}
    MoveOnly(MoveOnly&& other) : Movable(std::move(other)) {}
};

void Sleep ( int ms ) {
    std::chrono::milliseconds sleep_time( ms );
    std::this_thread::sleep_for( sleep_time );
}

template<typename T, typename U>
auto max(const T& left, const U& right) -> decltype(left > right ? left : right)
{
    return left > right ? left : right;
}
template<typename T, typename U>
auto min(const T& left, const U& right) -> decltype(left < right ? left : right)
{
    return left < right ? left : right;
}

template<typename T, std::size_t N>
inline std::size_t array_length(const T(&)[N]) {
    return N;
}

// TODO: consider adding a common comparator with member functions is_equal, is_less, is_greater, etc.
struct IsEqual {
        template <typename T>
        static bool compare( const std::weak_ptr<T> &t1, const std::weak_ptr<T> &t2 ) {
            // Compare real pointers.
            return t1.lock().get() == t2.lock().get();
        }

        template <typename T>
        static bool compare( const std::unique_ptr<T> &t1, const std::unique_ptr<T> &t2 ) {
            // Compare real values.
            return *t1 == *t2;
        }
        template <typename T1, typename T2>
        static bool compare( const std::pair< const std::weak_ptr<T1>, std::weak_ptr<T2> > &t1,
                const std::pair< const std::weak_ptr<T1>, std::weak_ptr<T2> > &t2 ) {
            // Compare real pointers.
            return t1.first.lock().get() == t2.first.lock().get() &&
                t1.second.lock().get() == t2.second.lock().get();
        }
        template <typename T1, typename T2>
        static bool compare( const T1 &t1, const T2 &t2 ) {
            return t1 == t2;
        }
        template <typename T1, typename T2>
        bool operator()( T1 &t1, T2 &t2) const {
            return compare( (const T1&)t1, (const T2&)t2 );
        }
}; // struct IsEqual

template <typename T, std::size_t N>
tbb::blocked_range<T*> make_blocked_range( T(& array)[N] ) {
    return tbb::blocked_range<T*>(array, array + N);
}

template <typename T>
void check_range_bounds_after_splitting( const tbb::blocked_range<T>& original, const tbb::blocked_range<T>& first,
                                         const tbb::blocked_range<T>& second, const T& expected_first_end )
{
    REQUIRE(first.begin() == original.begin());
    REQUIRE(first.end() == expected_first_end);
    REQUIRE(second.begin() == expected_first_end);
    REQUIRE(second.end() == original.end());
    REQUIRE(first.size() + second.size() == original.size());
}

template<typename M>
struct Counter {
    using mutex_type = M;
    M mutex;
    volatile long value;
};

template<typename M>
struct AtomicCounter {
    using mutex_type = M;
    M mutex;
    std::atomic<long> value;
};

#if __TBB_CPP20_CONCEPTS_PRESENT
template <template <typename...> class Template, typename... Types>
concept well_formed_instantiation = requires {
    typename Template<Types...>;
};
#endif // __TBB_CPP20_CONCEPTS_PRESENT

class LifeTrackableObject {
    using set_type = std::unordered_set<const LifeTrackableObject*>;
    static set_type alive_objects;
public:
    LifeTrackableObject() {
        alive_objects.insert(this);
    }

    LifeTrackableObject(const LifeTrackableObject&) {
        alive_objects.insert(this);
    }

    LifeTrackableObject(LifeTrackableObject&&) {
        alive_objects.insert(this);
    }

    LifeTrackableObject& operator=(const LifeTrackableObject&) = default;
    LifeTrackableObject& operator=(LifeTrackableObject&&) = default;

    ~LifeTrackableObject() {
        alive_objects.erase(this);
    }

    static bool is_alive(const LifeTrackableObject& object) {
        return is_alive(&object);
    }

    static bool is_alive(const LifeTrackableObject* object) {
        return alive_objects.find(object) != alive_objects.end();
    }

    static const set_type& set() {
        return alive_objects;
    }
};
std::unordered_set<const LifeTrackableObject*> LifeTrackableObject::alive_objects{};

} // namespace utils

#endif // __TBB_test_common_utils_H
