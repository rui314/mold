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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#define TBB_DEFINE_STD_HASH_SPECIALIZATIONS 1
#include <tbb/concurrent_unordered_set.h>
#include "common/concurrent_unordered_common.h"

//! \file test_concurrent_unordered_set.cpp
//! \brief Test for [containers.concurrent_unordered_set containers.concurrent_unordered_multiset] specifications

template <typename... Args>
struct AllowMultimapping<tbb::concurrent_unordered_multiset<Args...>> : std::true_type {};

template <typename Value>
using MyAllocator = LocalCountingAllocator<std::allocator<Value>>;

using move_support_tests::FooWithAssign;

using set_type = tbb::concurrent_unordered_set<int, std::hash<int>, std::equal_to<int>, MyAllocator<int>>;
using multiset_type = tbb::concurrent_unordered_multiset<int, std::hash<int>, std::equal_to<int>, MyAllocator<int>>;
using degenerate_set_type = tbb::concurrent_unordered_set<int, degenerate_hash<int>, std::equal_to<int>, MyAllocator<int>>;
using degenerate_multiset_type = tbb::concurrent_unordered_multiset<int, degenerate_hash<int>, std::equal_to<int>, MyAllocator<int>>;

using checked_set_type = tbb::concurrent_unordered_set<CheckType<int>, std::hash<CheckType<int>>, std::equal_to<CheckType<int>>, MyAllocator<CheckType<int>>>;
using checked_multiset_type = tbb::concurrent_unordered_multiset<CheckType<int>, std::hash<CheckType<int>>,
                                                                 std::equal_to<CheckType<int>>, MyAllocator<CheckType<int>>>;
using checked_state_set_type = tbb::concurrent_unordered_set<FooWithAssign, std::hash<FooWithAssign>, std::equal_to<FooWithAssign>,
                                                             MyAllocator<FooWithAssign>>;
using checked_state_multiset_type = tbb::concurrent_unordered_multiset<FooWithAssign, std::hash<FooWithAssign>, std::equal_to<FooWithAssign>,
                                                                       MyAllocator<FooWithAssign>>;

struct CusetTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_unordered_set<T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct CusetTraits

struct CumultisetTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_unordered_multiset<T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct CumultisetTraits

struct UnorderedSetTypesTester {
    template <bool DefCtorPresent, typename ValueType>
    void check( const std::list<ValueType>& lst ) {
        TypeTester<DefCtorPresent, tbb::concurrent_unordered_set<ValueType, std::hash<ValueType>, utils::IsEqual>>(lst);
        TypeTester<DefCtorPresent, tbb::concurrent_unordered_multiset<ValueType, std::hash<ValueType>, utils::IsEqual>>(lst);
    }
};

void test_specific_types() {
    test_set_specific_types<UnorderedSetTypesTester>();

    // Regressiong test for a problem with excessive requirements of emplace()
    test_emplace_insert<tbb::concurrent_unordered_set<test::unique_ptr<int>>,
                        std::false_type>(new int, new int);
    test_emplace_insert<tbb::concurrent_unordered_multiset<test::unique_ptr<int>>,
                        std::false_type>(new int, new int);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("basic test for concurrent_unordered_set with degenerate hash") {
    test_basic<degenerate_set_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("basic test for concurrent_unordered_multiset with degenerate hash") {
    test_basic<degenerate_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_set with elements ctor and dtor check") {
    Checker<checked_set_type::value_type> checker;
    test_basic<checked_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_multiset with elements ctor and dtor check") {
    Checker<checked_multiset_type::value_type> checker;
    test_basic<checked_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_set with elements state check") {
    test_basic<checked_state_set_type, /*CheckState = */std::true_type>();
}

//! \brief \ref resource_usage
TEST_CASE("basic test for concurrent_unordered_multiset with elements state check") {
    test_basic<checked_state_multiset_type, /*CheckState = */std::true_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_set with degenerate hash") {
    test_concurrent<degenerate_set_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multiset with degenerate hash") {
    test_concurrent<degenerate_multiset_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multiset with no unique keys") {
    test_concurrent<multiset_type>(true);
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("multithreading support in concurrent_unordered_multiset with degenerate hash and no unique keys") {
    test_concurrent<degenerate_multiset_type>(true);
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_set with elements ctor and dtor check") {
    Checker<checked_set_type::value_type> checker;
    test_concurrent<checked_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_multiset with elements ctor and dtor check") {
    Checker<checked_multiset_type::value_type> checker;
    test_concurrent<checked_multiset_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_set with elements state check") {
    test_concurrent<checked_state_set_type>();
}

//! \brief \ref resource_usage
TEST_CASE("multithreading support in concurrent_unordered_multiset with elements state check") {
    test_concurrent<checked_state_multiset_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_unordered_set") {
    test_range_based_for_support<set_type>();
}

//! \brief \ref interface \ref error_guessing
TEST_CASE("range based for support in concurrent_unordered_multiset") {
    test_range_based_for_support<multiset_type>();
}

//! \brief \ref stress \ref error_guessing
TEST_CASE("merge and concurrent merge in concurrent_unordered_set and set with degenerate hash") {
    node_handling_tests::test_merge<set_type, degenerate_set_type>(1000);
}

//! \brief \ref regression
TEST_CASE("concurrent_unordered_set/multiset with specific key types") {
    test_specific_types();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_set with std::scoped_allocator_adaptor") {
    test_scoped_allocator<CusetTraits>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_multiset with std::scoped_allocator_adaptor") {
    test_scoped_allocator<CumultisetTraits>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_set::swap with not always equal allocator") {
    using not_always_equal_alloc_set_type = tbb::concurrent_unordered_set<int, std::hash<int>, std::equal_to<int>,
                                                                          NotAlwaysEqualAllocator<int>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_multiset::swap with not always equal allocator") {
    using not_always_equal_alloc_mset_type = tbb::concurrent_unordered_multiset<int, std::hash<int>, std::equal_to<int>,
                                                                                NotAlwaysEqualAllocator<int>>;
    test_swap_not_always_equal_allocator<not_always_equal_alloc_mset_type>();
}

#if __TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_set throwing copy constructor") {
    using exception_set_type = tbb::concurrent_unordered_set<ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_set_type>();
}

//! \brief \ref error_guessing
TEST_CASE("concurrent_unordered_multimap throwing copy constructor") {
    using exception_mset_type = tbb::concurrent_unordered_multiset<ThrowOnCopy>;
    test_exception_on_copy_ctor<exception_mset_type>();
}
#endif // __TBB_USE_EXCEPTIONS

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_unordered_set ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_set<int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_set<int>::const_range_type>);
}

//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_unordered_multiset ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_multiset<int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_unordered_multiset<int>::const_range_type>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
