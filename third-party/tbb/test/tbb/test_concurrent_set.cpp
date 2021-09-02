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
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <tbb/concurrent_set.h>
#include "common/concurrent_ordered_common.h"

//! \file test_concurrent_set.cpp
//! \brief Test for [containers.concurrent_set containers.concurrent_multiset] specifications

template <typename... Args>
struct AllowMultimapping<tbb::concurrent_multiset<Args...>> : std::true_type {};

template <typename Key>
using MyAllocator = LocalCountingAllocator<std::allocator<Key>>;

using move_support_tests::FooWithAssign;

using set_type = tbb::concurrent_set<int, std::less<int>, MyAllocator<int>>;
using multiset_type = tbb::concurrent_multiset<int, std::less<int>, MyAllocator<int>>;
using checked_set_type = tbb::concurrent_set<CheckType<int>, std::less<CheckType<int>>, MyAllocator<CheckType<int>>>;
using checked_multiset_type = tbb::concurrent_multiset<CheckType<int>, std::less<CheckType<int>>, MyAllocator<CheckType<int>>>;
using greater_set_type = tbb::concurrent_set<int, std::greater<int>, MyAllocator<int>>;
using greater_multiset_type = tbb::concurrent_multiset<int, std::greater<int>, MyAllocator<int>>;
using checked_state_set_type = tbb::concurrent_set<FooWithAssign, std::less<FooWithAssign>, MyAllocator<FooWithAssign>>;
using checked_state_multiset_type = tbb::concurrent_multiset<FooWithAssign, std::less<FooWithAssign>, MyAllocator<FooWithAssign>>;

struct COSetTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_set<T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct COSetTraits

struct COMultisetTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_multiset<T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct COMultisetTraits

struct OrderedSetTypesTester {
    template <bool DefCtorPresent, typename ValueType>
    void check( const std::list<ValueType>& lst ) {
        TypeTester<DefCtorPresent, tbb::concurrent_set<ValueType>>(lst);
        TypeTester<DefCtorPresent, tbb::concurrent_multiset<ValueType>>(lst);
    }
}; // struct OrderedMapTypesTester

void test_specific_types() {
    test_set_specific_types<OrderedSetTypesTester>();

    // Regression test for a problem with excessive requirements of emplace
    test_emplace_insert<tbb::concurrent_set<test::unique_ptr<int>>, std::false_type>
                       (new int, new int);
    test_emplace_insert<tbb::concurrent_multiset<test::unique_ptr<int>>, std::false_type>
                       (new int, new int);
}

// Regression test for an issue in lock free algorithms
// In some cases this test hangs due to broken skip list internal structure on levels > 1
// This issue was resolved by adding index_number into the skip list node
void test_cycles_absense() {
    for (std::size_t execution = 0; execution != 10; ++execution) {
        tbb::concurrent_multiset<int> mset;
        std::vector<int> v(2);
        int vector_size = int(v.size());

        for (int i = 0; i != vector_size; ++i) {
            v[i] = int(i);
        }
        size_t num_threads = 4; // Can be changed to 2 for debugging

        utils::NativeParallelFor(num_threads, [&](size_t) {
            for (int i = 0; i != vector_size; ++i) {
                mset.emplace(i);
            }
        });

        for (int i = 0; i != vector_size; ++i) {
            REQUIRE(mset.count(i) == num_threads);
        }
    }
}

//! \brief \ref error_guessing
TEST_CASE("basic test for concurrent_set with greater compare") {
    test_basic<greater_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("basic test for concurrent_multiset with greater compare") {
    test_basic<greater_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_set with elements ctor and dtor check") {
    Checker<checked_set_type::value_type> checker;
    test_basic<checked_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_multiset with elements ctor and dtor check") {
    Checker<checked_multiset_type::value_type> checker;
    test_basic<checked_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_set with elements state check") {
    test_basic<checked_state_set_type, /*CheckState = */std::true_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_multiset with elements state check") {
    test_basic<checked_state_multiset_type, /*CheckState = */std::true_type>();
}

//! \brief \ref error_guessing
TEST_CASE("multithreading support in concurrent_set with greater compare") {
    test_concurrent<greater_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("multithreading support in concurrent_multiset with greater compare") {
    test_concurrent<greater_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreding support in concurrent_set with elements ctor and dtor check") {
    Checker<checked_set_type::value_type> checker;
    test_concurrent<checked_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_multiset with elements ctor and dtor check") {
    Checker<checked_multiset_type::value_type> checker;
    test_concurrent<checked_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_set with elements state check") {
    test_concurrent<checked_state_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_multiset with elements state check") {
    test_concurrent<checked_state_multiset_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_multiset with no unique keys") {
    test_concurrent<multiset_type>(true);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_multiset with greater compare and no unique keys") {
    test_concurrent<greater_multiset_type>(true);
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_set") {
    test_range_based_for_support<set_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_multiset") {
    test_range_based_for_support<multiset_type>();
}

//! \brief \ref regression
TEST_CASE("concurrent set/multiset with specific key types") {
    test_specific_types();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_set with std::scoped_allocator_adaptor") {
    test_scoped_allocator<COSetTraits>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_multiset with std::scoped_allocator_adaptor") {
    test_scoped_allocator<COMultisetTraits>();
}

//! \brief \ref regression
TEST_CASE("broken internal structure for multiset") {
    test_cycles_absense();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_set::swap with not always equal allocator") {
    using not_always_equal_alloc_set_type = tbb::concurrent_set<int, std::less<int>, NotAlwaysEqualAllocator<int>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_multiset::swap with not always equal allocator") {
    using not_always_equal_alloc_mset_type = tbb::concurrent_multiset<int, std::less<int>, NotAlwaysEqualAllocator<int>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_mset_type>();
}

#if TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("concurrent_set throwing copy constructor") {
    using exception_set_type = tbb::concurrent_set<ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_multiset throwing copy constructor") {
    using exception_mset_type = tbb::concurrent_multiset<ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_mset_type>();
}
#endif // TBB_USE_EXCEPTIONS

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_set ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_set<int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_set<int>::const_range_type>);
}

//! \brief \ref error_guessing
TEST_CASE("container range concept for concurrent_multiset ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_multiset<int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_multiset<int>::const_range_type>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
