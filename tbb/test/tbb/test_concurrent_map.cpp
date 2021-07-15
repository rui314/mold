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
#include <tbb/concurrent_map.h>
#include "common/concurrent_ordered_common.h"

//! \file test_concurrent_map.cpp
//! \brief Test for [containers.concurrent_map containers.concurrent_multimap] specifications

template <typename... Args>
struct AllowMultimapping<tbb::concurrent_multimap<Args...>> : std::true_type {};

template <typename Key, typename Mapped>
using MyAllocator = LocalCountingAllocator<std::allocator<std::pair<const Key, Mapped>>>;

using move_support_tests::FooWithAssign;

using map_type = tbb::concurrent_map<int, int, std::less<int>, MyAllocator<int, int>>;
using multimap_type = tbb::concurrent_multimap<int, int, std::less<int>, MyAllocator<int, int>>;
using checked_map_type = tbb::concurrent_map<int, CheckType<int>, std::less<int>, MyAllocator<int, CheckType<int>>>;
using checked_multimap_type = tbb::concurrent_multimap<int, CheckType<int>, std::less<int>, MyAllocator<int, CheckType<int>>>;
using greater_map_type = tbb::concurrent_map<int, int, std::greater<int>, MyAllocator<int, int>>;
using greater_multimap_type = tbb::concurrent_multimap<int, int, std::greater<int>, MyAllocator<int, int>>;
using checked_state_map_type = tbb::concurrent_map<intptr_t, FooWithAssign, std::less<intptr_t>,
                                                   MyAllocator<intptr_t, FooWithAssign>>;
using checked_state_multimap_type = tbb::concurrent_multimap<intptr_t, FooWithAssign, std::less<intptr_t>,
                                                             MyAllocator<intptr_t, FooWithAssign>>;

template <>
struct SpecialTests<map_type> {
    static void Test() {
        SpecialMapTests<map_type>();
    }
};

template <>
struct SpecialTests<multimap_type> {
    static void Test() {
        SpecialMultiMapTests<multimap_type>();
    }
};

struct COMapTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_map<T, T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct COMapTraits

struct COMultimapTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_multimap<T, T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct COMultimapTraits

struct OrderedMapTypesTester {
    template <bool DefCtorPresent, typename ValueType>
    void check( const std::list<ValueType>& lst ) {
        using key_type = typename ValueType::first_type;
        using mapped_type = typename ValueType::second_type;

        TypeTester<DefCtorPresent, tbb::concurrent_map<key_type, mapped_type>>(lst);
        TypeTester<DefCtorPresent, tbb::concurrent_multimap<key_type, mapped_type>>(lst);
    }
}; // struct OrderedMapTypesTester

void test_specific_types() {
    test_map_specific_types<OrderedMapTypesTester>();

    // Regression test for a problem with excessive requirements of emplace()
    test_emplace_insert<tbb::concurrent_map<int*, test::unique_ptr<int>>,std::false_type>
                       (new int, new int);
    test_emplace_insert<tbb::concurrent_multimap<int*, test::unique_ptr<int>>,std::false_type>
                       (new int, new int);
}

// Regression test for an issue in lock free algorithms
// In some cases this test hangs due to broken skip list internal structure on levels > 1
// This issue was resolved by adding index_number into the skip list node
void test_cycles_absense() {
    for (std::size_t execution = 0; execution != 10; ++execution) {
        tbb::concurrent_multimap<int, int> mmap;
        std::vector<int> v(2);
        int vector_size = int(v.size());

        for (int i = 0; i != vector_size; ++i) {
            v[i] = i;
        }
        size_t num_threads = 4; // Can be changed to 2 for debugging

        utils::NativeParallelFor(num_threads, [&](size_t) {
            for (int i = 0; i != vector_size; ++i) {
                mmap.emplace(i, i);
            }
        });

        for (int i = 0; i != vector_size; ++i) {
            REQUIRE(mmap.count(i) == num_threads);
        }
    }
}

//! \brief \ref error_guessing
TEST_CASE("basic test for concurrent_map with greater compare") {
    test_basic<greater_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("basic test for concurrent_multimap with greater compare") {
    test_basic<greater_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_map with elements ctor and dtor check") {
    Checker<checked_map_type::value_type> checker;
    test_basic<checked_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_multimap with elements ctor and dtor check") {
    Checker<checked_multimap_type::value_type> checker;
    test_basic<checked_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_map with elements state check") {
    test_basic<checked_state_map_type, /*CheckState = */std::true_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_multimap with elements state check") {
    test_basic<checked_state_multimap_type, /*CheckState = */std::true_type>();
}

//! \brief \ref error_guessing
TEST_CASE("multithreading support in concurrent_map with greater compare") {
    test_concurrent<greater_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("multithreading support in concurrent_multimap with greater compare") {
    test_concurrent<greater_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_map with elements ctor and dtor check") {
    Checker<checked_map_type::value_type> checker;
    test_concurrent<checked_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_multimap with elements ctor and dtor check") {
    Checker<checked_multimap_type::value_type> checker;
    test_concurrent<checked_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_map with elements state check") {
    test_concurrent<checked_state_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_multimap with elements state check") {
    test_concurrent<checked_state_multimap_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_multimap no unique keys") {
    test_concurrent<multimap_type>(true);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_multimap with greater compare and no unique keys") {
    test_concurrent<greater_multimap_type>(true);
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_map") {
    test_range_based_for_support<map_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_multimap") {
    test_range_based_for_support<multimap_type>();
}

//! \brief \ref regression
TEST_CASE("concurrent_map/multimap with specific key/mapped types") {
    test_specific_types();
}

// TODO: add test with scoped_allocator_adaptor with broken macro

//! \brief \ref regression
TEST_CASE("broken internal structure for multimap") {
    test_cycles_absense();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_map::swap with not always equal allocator") {
    using not_always_equal_alloc_map_type = tbb::concurrent_map<int, int, std::less<int>,
                                                                NotAlwaysEqualAllocator<std::pair<const int, int>>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_multimap::swap with not always equal allocator") {
    using not_always_equal_alloc_mmap_type = tbb::concurrent_multimap<int, int, std::less<int>,
                                                                      NotAlwaysEqualAllocator<std::pair<const int, int>>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_mmap_type>();
}

#if TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("concurrent_map throwing copy constructor") {
    using exception_map_type = tbb::concurrent_map<ThrowOnCopy, ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_multimap throwing copy constructor") {
    using exception_mmap_type = tbb::concurrent_multimap<ThrowOnCopy, ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_mmap_type>();
}
#endif // TBB_USE_EXCEPTIONS

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_map ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_map<int, int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_map<int, int>::const_range_type>);
}

//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_multimap ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_multimap<int, int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_multimap<int, int>::const_range_type>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
