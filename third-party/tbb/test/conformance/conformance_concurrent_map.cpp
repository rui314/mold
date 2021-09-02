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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include "oneapi/tbb/concurrent_map.h"
#include <common/test.h>
#include <common/utils.h>
#include <common/concurrent_ordered_common.h>
#include <memory>
#include <type_traits>

//! \file conformance_concurrent_map.cpp
//! \brief Test for [containers.concurrent_map containers.concurrent_multimap] specifications

template <typename... Args>
struct AllowMultimapping<oneapi::tbb::concurrent_multimap<Args...>> : std::true_type {};

template <typename Key, typename Mapped>
using Allocator = LocalCountingAllocator<std::allocator<std::pair<const Key, Mapped>>>;

using map_type = oneapi::tbb::concurrent_map<int, int, std::less<int>, Allocator<int, int>>;
using multimap_type = oneapi::tbb::concurrent_multimap<int, int, std::less<int>, Allocator<int, int>>;

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

template <template <typename...> class ContainerType>
void test_member_types() {
    using default_container_type = ContainerType<int, int>;
    static_assert(std::is_same<typename default_container_type::key_compare, std::less<int>>::value,
                  "Incorrect default template comparator");
    static_assert(std::is_same<typename default_container_type::allocator_type, oneapi::tbb::tbb_allocator<std::pair<const int, int>>>::value,
                  "Incorrect default template allocator");

    auto test_comparator = [](const int&, const int&)->bool { return true; };
    using test_allocator_type = std::allocator<std::pair<const int, int>>;

    using container_type = ContainerType<int, int, decltype(test_comparator), test_allocator_type>;

    static_assert(std::is_same<typename container_type::key_type, int>::value,
                  "Incorrect container key_type member type");
    static_assert(std::is_same<typename container_type::mapped_type, int>::value,
                  "Incorrect container mapped_type member type");
    static_assert(std::is_same<typename container_type::value_type, std::pair<const int, int>>::value,
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
template <template <typename...> typename TMap>
void test_deduction_guides() {
    std::vector<std::pair<int, int>> v(10, {0, 0});
    TMap map(v.begin(), v.end());
    static_assert(std::is_same_v<decltype(map), TMap<int, int> >, "WRONG\n");

    std::greater<int> compare;
    std::allocator<std::pair<const int, int>> allocator;
    TMap map2(v.begin(), v.end(), compare);
    static_assert(std::is_same_v<decltype(map2), TMap<int, int, decltype(compare)> >, "WRONG\n");

    TMap map3(v.begin(), v.end(), allocator);
    static_assert(std::is_same_v<decltype(map3), TMap<int, int, std::less<int>, decltype(allocator)> >, "WRONG\n");

    TMap map4(v.begin(), v.end(), compare, allocator);
    static_assert(std::is_same_v<decltype(map4), TMap<int, int, decltype(compare), decltype(allocator)> >, "WRONG\n");

    using pair_t = std::pair<const int, int>;
    auto init = { pair_t{1, 1}, pair_t{2, 2}, pair_t{3, 3} };
    TMap map5(init);
    static_assert(std::is_same_v<decltype(map5), TMap<int, int> >, "WRONG\n");

    TMap map6(init, compare);
    static_assert(std::is_same_v<decltype(map6), TMap<int, int, decltype(compare)> >, "WRONG\n");

    TMap map7(init, allocator);
    static_assert(std::is_same_v<decltype(map7), TMap<int, int, std::less<int>, decltype(allocator)> >, "WRONG\n");

    TMap map8(init, compare, allocator);
    static_assert(std::is_same_v<decltype(map8), TMap<int, int, decltype(compare), decltype(allocator)> >, "WRONG\n");
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

template <template <typename...> class MapType>
void test_heterogeneous_functions() {
    check_heterogeneous_functions_key_int<MapType, int, int>();
    check_heterogeneous_functions_key_string<MapType, std::string, std::string>();
    check_heterogeneous_bound_functions<MapType<int, int, TransparentLess>>();
}

struct COMapTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_map<T, T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct COMapTraits

struct COMultimapTraits : OrderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_multimap<T, T, std::less<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct COMultimapTraits

//! Testing concurrent_map member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_map member types") {
    test_member_types<oneapi::tbb::concurrent_map>();
}

//! Testing requirements of concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_map requirements") {
    test_basic<map_type>();
}

//! Testing multithreading support in concurrent_map
//! \brief \ref requirement
TEST_CASE("concurrent_map multithreading support") {
    test_concurrent<map_type>();
}

//! Testing move constructors and assignment operator in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_map move semantics support") {
    test_rvalue_ref_support<COMapTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_map") {
    test_initializer_list_support<map_type>({{1, 1}, {2, 2}, {3, 3}, {4, 4}});
}

//! Testing node handling in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_map") {
    node_handling_tests::test_node_handling_support<map_type>();
}

//! Testing std::allocator_traits support in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_map") {
    test_allocator_traits_support<COMapTraits>();
}

//! Testing heterogeneous overloads in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_map") {
    test_heterogeneous_functions<oneapi::tbb::concurrent_map>();
}

//! Testing insert overloads with generic pair in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("insertion by generic pair in concurrent_map") {
    test_insert_by_generic_pair<oneapi::tbb::concurrent_map>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_map") {
    test_deduction_guides<oneapi::tbb::concurrent_map>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing comparisons of concurrent_map
//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_map comparisons") {
    test_map_comparisons<oneapi::tbb::concurrent_map>();
}

//! Testing concurrent_multimap member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multimap member types") {
    test_member_types<oneapi::tbb::concurrent_multimap>();
}

//! Testing requirements of concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multimap requirements") {
    test_basic<multimap_type>();
}

//! Testing multithreading support in concurrent_multimap
//! \brief \ref requirement
TEST_CASE("concurrent_multimap multithreading support") {
    test_concurrent<multimap_type>();
}

//! Testing move constructors and assignment operator in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_multimap multithreading support") {
    test_rvalue_ref_support<COMultimapTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_multimap") {
    test_initializer_list_support<multimap_type>({{1, 1}, {2, 2}, {3, 3}, {4, 4}, {4, 40}});
}

//! Testing node handling support in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_multimap") {
    node_handling_tests::test_node_handling_support<multimap_type>();
}

//! Testing std::allocator_traits support in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_multimap") {
    test_allocator_traits_support<COMultimapTraits>();
}

//! Testing heterogeneous overloads in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_multimap") {
    test_heterogeneous_functions<oneapi::tbb::concurrent_multimap>();
}

//! Testing insert overloads with generic pair in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("insertion by generic pair in concurrent_multimap") {
    test_insert_by_generic_pair<oneapi::tbb::concurrent_multimap>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_multimap") {
    test_deduction_guides<oneapi::tbb::concurrent_multimap>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing comparison operators in concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_multimap comparisons") {
    test_map_comparisons<oneapi::tbb::concurrent_multimap>();
}

//! Testing of merge operations in concurrent_map and concurrent_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("merge operations") {
    node_handling_tests::test_merge<map_type, multimap_type>(1000);
}
