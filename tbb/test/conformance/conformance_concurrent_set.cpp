/*
    Copyright (c) 2019-2021 Intel Corporation

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
#include "oneapi/tbb/concurrent_set.h"
#include <common/test.h>
#include <common/utils.h>
#include <common/concurrent_ordered_common.h>
#include <memory>
#include <type_traits>

//! \file conformance_concurrent_set.cpp
//! \brief Test for [containers.concurrent_set containers.concurrent_multiset] specifications

template <typename... Args>
struct AllowMultimapping<oneapi::tbb::concurrent_multiset<Args...>> : std::true_type {};

template <typename Key>
using Allocator = LocalCountingAllocator<std::allocator<Key>>;

using set_type = oneapi::tbb::concurrent_set<int, std::less<int>, Allocator<int>>;
using multiset_type = oneapi::tbb::concurrent_multiset<int, std::less<int>, Allocator<int>>;

template <template <typename...> class ContainerType>
void test_member_types() {
    using default_container_type = ContainerType<int>;
    static_assert(std::is_same<typename default_container_type::key_compare, std::less<int>>::value,
                  "Incorrect default template comparator");
    static_assert(std::is_same<typename default_container_type::allocator_type, oneapi::tbb::tbb_allocator<int>>::value,
                  "Incorrect default template allocator");

    auto test_comparator = [](const int&, const int&)->bool { return true; };
    using test_allocator_type = std::allocator<int>;

    using container_type = ContainerType<int, decltype(test_comparator), test_allocator_type>;

    static_assert(std::is_same<typename container_type::key_type, int>::value,
                  "Incorrect container key_type member type");
    static_assert(std::is_same<typename container_type::value_type, int>::value,
                  "Incorrect container value_type member type");

    static_assert(std::is_unsigned<typename container_type::size_type>::value,
                  "Incorrect container size_type member type");
    static_assert(std::is_signed<typename container_type::difference_type>::value,
                  "Incorrect container difference_type member type");

    static_assert(std::is_same<typename container_type::key_compare, decltype(test_comparator)>::value,
                  "Incorrect container key_compare member type");
    static_assert(std::is_same<typename container_type::allocator_type, test_allocator_type>::value,
                  "Incorrect container allocator_type member type");

    using value_type = typename container_type::value_type;
    static_assert(std::is_same<typename container_type::reference, value_type&>::value,
                  "Incorrect container reference member type");
    static_assert(std::is_same<typename container_type::const_reference, const value_type&>::value,
                  "Incorrect container const_reference member type");
    using allocator_type = typename container_type::allocator_type;
    static_assert(std::is_same<typename container_type::pointer, typename std::allocator_traits<allocator_type>::pointer>::value,
                  "Incorrect container pointer member type");
    static_assert(std::is_same<typename container_type::const_pointer, typename std::allocator_traits<allocator_type>::const_pointer>::value,
                  "Incorrect container const_pointer member type");

    static_assert(utils::is_forward_iterator<typename container_type::iterator>::value,
                  "Incorrect container iterator member type");
    static_assert(!std::is_const<typename container_type::iterator::value_type>::value,
                  "Incorrect container iterator member type");
    static_assert(utils::is_forward_iterator<typename container_type::const_iterator>::value,
                  "Incorrect container const_iterator member type");
    static_assert(std::is_const<typename container_type::const_iterator::value_type>::value,
                  "Incorrect container const_iterator member type");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <template <typename ...> typename TSet>
void test_deduction_guides() {
    std::vector<int> vc({1, 2, 3});
    TSet set(vc.begin(), vc.end());
    static_assert(std::is_same_v<decltype(set), TSet<int>>, "Wrong");

    std::greater<int> compare;
    std::allocator<int> allocator;

    TSet set2(vc.begin(), vc.end(), compare);
    static_assert(std::is_same_v<decltype(set2), TSet<int, decltype(compare)>>, "Wrong");

    TSet set3(vc.begin(), vc.end(), allocator);
    static_assert(std::is_same_v<decltype(set3), TSet<int, std::less<int>, decltype(allocator)>>, "Wrong");

    TSet set4(vc.begin(), vc.end(), compare, allocator);
    static_assert(std::is_same_v<decltype(set4), TSet<int, decltype(compare), decltype(allocator)>>, "Wrong");

    auto init_list = { int(1), int(2), int(3) };

    TSet set5(init_list);
    static_assert(std::is_same_v<decltype(set5), TSet<int>>, "Wrong");

    TSet set6(init_list, compare);
    static_assert(std::is_same_v<decltype(set6), TSet<int, decltype(compare)>>, "Wrong");

    TSet set7(init_list, allocator);
    static_assert(std::is_same_v<decltype(set7), TSet<int, std::less<int>, decltype(allocator)>>, "Wrong");

    TSet set8(init_list, compare, allocator);
    static_assert(std::is_same_v<decltype(set8), TSet<int, decltype(compare), decltype(allocator)>>, "Wrong");
}
#endif /*__TBB_CPP17_DEDUCTION_GUIDES_PRESENT*/

template <template <typename...> class SetType>
void test_heterogeneous_functions() {
    check_heterogeneous_functions_key_int<SetType, int>();
    check_heterogeneous_functions_key_string<SetType, std::string>();
    check_heterogeneous_bound_functions<SetType<int, TransparentLess>>();
}

struct COSetTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_set<T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct COSetTraits

struct COMultisetTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_multiset<T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct COMultisetTraits

//! Testing concurrent_set member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_set member types") {
    test_member_types<oneapi::tbb::concurrent_set>();
}

//! Testing multithreading support in concurrent_set
//! \brief \ref requirement
TEST_CASE("concurrent_set multithreading support") {
    test_concurrent<set_type>();
}

//! Testing move constructors and assignment operator in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_set move semantics support") {
    test_rvalue_ref_support<COSetTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_set") {
    test_initializer_list_support<set_type>({1, 2, 3, 4});
}

//! Testing node handling in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_set") {
    node_handling_tests::test_node_handling_support<set_type>();
}

//! Testing std::allocator_traits support in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_set") {
    test_allocator_traits_support<COSetTraits>();
}

//! Testing heterogeneous overloads in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_set") {
    test_heterogeneous_functions<oneapi::tbb::concurrent_set>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_set") {
    test_deduction_guides<oneapi::tbb::concurrent_set>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing comparison operators in concurrent_set
//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_set comparisons") {
    test_set_comparisons<oneapi::tbb::concurrent_set>();
}

//! Testing concurrent_multiset member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multiset member types") {
    test_member_types<oneapi::tbb::concurrent_multiset>();
}

//! Testing requirements of concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multiset requirements") {
    test_basic<multiset_type>();
}

//! Testing multithreading support in concurrent_multiset
//! \brief \ref requirement
TEST_CASE("concurrent_multiset multithreading support") {
    test_concurrent<multiset_type>();
}

//! Testing move constructors and assignment operator in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multiset multithreading support") {
    test_rvalue_ref_support<COMultisetTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_multimap") {
    test_initializer_list_support<multiset_type>({1, 2, 3, 4, 4});
}

//! Testing node handling support in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_multiset") {
    node_handling_tests::test_node_handling_support<multiset_type>();
}

//! Testing std::allocator_traits support in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_multiset") {
    test_allocator_traits_support<COMultisetTraits>();
}

//! Testing heterogeneous overloads in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_multiset") {
    test_heterogeneous_functions<oneapi::tbb::concurrent_multiset>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_multiset") {
    test_deduction_guides<oneapi::tbb::concurrent_multiset>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing comparison operators in concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_set comparisons") {
    test_set_comparisons<oneapi::tbb::concurrent_multiset>();
}

//! Testing of merge operations in concurrent_set and concurrent_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("merge operations") {
    node_handling_tests::test_merge<set_type, multiset_type>(1000);
}
