/*
    Copyright (c) 2005-2023 Intel Corporation

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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#define TBB_DEFINE_STD_HASH_SPECIALIZATIONS 1
#include <tbb/concurrent_unordered_map.h>
#include "common/concurrent_unordered_common.h"

//! \file test_concurrent_unordered_map.cpp
//! \brief Test for [containers.concurrent_unordered_map containers.concurrent_unordered_multimap] specifications

template <typename... Args>
struct AllowMultimapping<tbb::concurrent_unordered_multimap<Args...>> : std::true_type {};

template <typename Key, typename Mapped>
using MyAllocator = LocalCountingAllocator<std::allocator<std::pair<const Key, Mapped>>>;

using move_support_tests::FooWithAssign;

using map_type = tbb::concurrent_unordered_map<int, int, std::hash<int>, std::equal_to<int>, MyAllocator<int, int>>;
using multimap_type = tbb::concurrent_unordered_multimap<int, int, std::hash<int>, std::equal_to<int>, MyAllocator<int, int>>;
using degenerate_map_type = tbb::concurrent_unordered_map<int, int, degenerate_hash<int>, std::equal_to<int>, MyAllocator<int, int>>;
using degenerate_multimap_type = tbb::concurrent_unordered_multimap<int, int, degenerate_hash<int>, std::equal_to<int>, MyAllocator<int, int>>;

using checked_map_type = tbb::concurrent_unordered_map<int, CheckType<int>, std::hash<int>, std::equal_to<int>, MyAllocator<int, CheckType<int>>>;
using checked_multimap_type = tbb::concurrent_unordered_multimap<int, CheckType<int>, std::hash<int>, std::equal_to<int>, MyAllocator<int, CheckType<int>>>;
using checked_state_map_type = tbb::concurrent_unordered_map<intptr_t, FooWithAssign, std::hash<intptr_t>,
                                                             std::equal_to<intptr_t>, MyAllocator<intptr_t, FooWithAssign>>;
using checked_state_multimap_type = tbb::concurrent_unordered_multimap<intptr_t, FooWithAssign, std::hash<intptr_t>,
                                                                       std::equal_to<intptr_t>, MyAllocator<intptr_t, FooWithAssign>>;

struct CumapTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_unordered_map<T, T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct CumapTraits

struct CumultimapTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_unordered_multimap<T, T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct CumultimapTraits

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

struct UnorderedMapTypesTester {
    template <template <typename...> class GeneralTableType, typename Key, typename Mapped>
    using table_type = GeneralTableType<Key, Mapped, std::hash<Key>, utils::IsEqual>;

    template <bool DefCtorPresent, typename ValueType>
    void check( const std::list<ValueType>& lst ) {
        using key_type = typename std::remove_const<typename ValueType::first_type>::type;
        using mapped_type = typename ValueType::second_type;

        TypeTester<DefCtorPresent, table_type<tbb::concurrent_unordered_map, key_type, mapped_type>>(lst);
        TypeTester<DefCtorPresent, table_type<tbb::concurrent_unordered_multimap, key_type, mapped_type>>(lst);
    }
}; // struct UnorderedMapTypesTester

void test_specific_types() {
    test_map_specific_types<UnorderedMapTypesTester>();

    // Regression test for a problem with excessive requirements of emplace()
    test_emplace_insert<tbb::concurrent_unordered_map<int*, test::unique_ptr<int>>, std::false_type>
                       (new int, new int);
    test_emplace_insert<tbb::concurrent_unordered_multimap<int*, test::unique_ptr<int>>, std::false_type>
                       (new int, new int);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("basic test for concurrent_unordered_map with degenerate hash") {
    test_basic<degenerate_map_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("basic test for concurrent_unordered_multimap with degenerate hash") {
    test_basic<degenerate_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_map with elements ctor and dtor check") {
    Checker<checked_map_type::mapped_type> checker;
    test_basic<checked_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_multimap with elements ctor and dtor check") {
    Checker<checked_multimap_type::mapped_type> checker;
    test_basic<checked_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_map with elements state check") {
    test_basic<checked_state_map_type, /*CheckState = */std::true_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_multimap with elements state check") {
    test_basic<checked_state_multimap_type, /*CheckState = */std::true_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_map with degenerate hash") {
    test_concurrent<degenerate_map_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multimap with degenerate hash") {
    test_concurrent<degenerate_multimap_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multimap no unique keys") {
    test_concurrent<multimap_type>(true);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multimap with degenerate hash and no unique keys") {
    test_concurrent<degenerate_multimap_type>(true);
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_map with elements ctor and dtor check") {
    Checker<checked_map_type::mapped_type> checker;
    test_concurrent<checked_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_multimap with elements ctor and dtor check") {
    Checker<checked_multimap_type::mapped_type> checker;
    test_concurrent<checked_multimap_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_map with elements state check") {
    test_concurrent<checked_state_map_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_multimap with elements state check") {
    test_concurrent<checked_state_multimap_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_unordered_map") {
    test_range_based_for_support<map_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_unordered_multimap") {
    test_range_based_for_support<multimap_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("merge and concurrent merge in concurrent_unordered_map with degenerative hash") {
    node_handling_tests::test_merge<map_type, degenerate_multimap_type>(1000);
}

//! \brief \ref regression
TEST_CASE("concurrent_unordered map/multimap with specific key/mapped types") {
    test_specific_types();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_map::swap with not always equal allocator") {
    using not_always_equal_alloc_map_type = tbb::concurrent_unordered_map<int, int, std::hash<int>, std::equal_to<int>,
                                                                          NotAlwaysEqualAllocator<std::pair<const int, int>>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_multimap::swap with not always equal allocator") {
    using not_always_equal_alloc_mmap_type = tbb::concurrent_unordered_multimap<int, int, std::hash<int>, std::equal_to<int>,
                                                                                NotAlwaysEqualAllocator<std::pair<const int, int>>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_mmap_type>();
}

#if TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_map throwing copy constructor") {
    using exception_map_type = tbb::concurrent_unordered_map<ThrowOnCopy, ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_map_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_multimap throwing copy constructor") {
    using exception_mmap_type = tbb::concurrent_unordered_multimap<ThrowOnCopy, ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_mmap_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_map whitebox throwing copy constructor") {
    using allocator_type = StaticSharedCountingAllocator<std::allocator<std::pair<const int, int>>>;
    using exception_mmap_type = tbb::concurrent_unordered_map<int, int, std::hash<int>, std::equal_to<int>, allocator_type>;

    exception_mmap_type map;
    for (int i = 0; i < 10; ++i) {
        map.insert(std::pair<const int, int>(i, 42));
    }

    allocator_type::set_limits(1);
    REQUIRE_THROWS_AS( [&] {
        exception_mmap_type map1(map);
        utils::suppress_unused_warning(map1);
    }(), const std::bad_alloc);
}

#endif // TBB_USE_EXCEPTIONS

// TODO: add test_scoped_allocator support with broken macro

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_unordered_map ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_map<int, int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_map<int, int>::const_range_type>);
}

//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_unordered_multimap ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_multimap<int, int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_multimap<int, int>::const_range_type>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

//! \brief \ref regression
TEST_CASE("reserve(0) issue regression test") {
    test_reserve_regression<oneapi::tbb::concurrent_unordered_map<int, int>>();
    test_reserve_regression<oneapi::tbb::concurrent_unordered_multimap<int, int>>();
}
