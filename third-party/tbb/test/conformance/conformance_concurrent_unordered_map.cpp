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

#include "oneapi/tbb/concurrent_unordered_map.h"
#include <common/test.h>
#include <common/utils.h>
#include <common/concurrent_unordered_common.h>
#include <memory>
#include <type_traits>

//! \file conformance_concurrent_unordered_map.cpp
//! \brief Test for [containers.concurrent_unordered_map containers.concurrent_unordered_multimap] specifications

template <typename... Args>
struct AllowMultimapping<oneapi::tbb::concurrent_unordered_multimap<Args...>> : std::true_type {};

template <typename Key, typename Mapped>
using Allocator = LocalCountingAllocator<std::allocator<std::pair<const Key, Mapped>>>;

using map_type = oneapi::tbb::concurrent_unordered_map<int, int, std::hash<int>, std::equal_to<int>, Allocator<int, int>>;
using multimap_type = oneapi::tbb::concurrent_unordered_multimap<int, int, std::hash<int>, std::equal_to<int>, Allocator<int, int>>;

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

template <template <typename... > class ContainerType>
void test_member_types() {
    using default_container_type = ContainerType<int, int>;
    static_assert(std::is_same<typename default_container_type::hasher, std::hash<int>>::value,
                  "Incorrect default template hasher");
    static_assert(std::is_same<typename default_container_type::key_equal, std::equal_to<int>>::value,
                  "Incorrect default template key equality");
    static_assert(std::is_same<typename default_container_type::allocator_type,
                               oneapi::tbb::tbb_allocator<std::pair<const int, int>>>::value,
                  "Incorrect default template allocator");

    auto test_hasher = [](const int&)->std::size_t { return 0; };
    auto test_equality = [](const int&, const int&)->bool { return true; };
    using test_allocator_type = std::allocator<std::pair<const int, int>>;

    using container_type = ContainerType<int, int, decltype(test_hasher),
                                         decltype(test_equality), test_allocator_type>;

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

    static_assert(std::is_same<typename container_type::hasher, decltype(test_hasher)>::value,
                  "Incorrect container hasher member type");
    static_assert(std::is_same<typename container_type::key_equal, decltype(test_equality)>::value,
                  "Incorrect container key_equal member type");

    using transparent_container_type = ContainerType<int, int, hasher_with_transparent_key_equal,
                                                     std::equal_to<int>, test_allocator_type>;

    static_assert(std::is_same<typename transparent_container_type::key_equal, transparent_key_equality>::value,
                  "Incorrect container key_equal member type");
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
                  "Incorrect container iterator member type");
    static_assert(utils::is_forward_iterator<typename container_type::local_iterator>::value,
                  "Incorrect container local_iterator member type");
    static_assert(!std::is_const<typename container_type::local_iterator::value_type>::value,
                  "Incorrect container local_iterator member type");
    static_assert(utils::is_forward_iterator<typename container_type::const_local_iterator>::value,
                  "Incorrect container const_local_iterator member type");
    static_assert(std::is_const<typename container_type::const_local_iterator::value_type>::value,
                  "Incorrect container const_local_iterator member type");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <template <typename...> typename TMap>
void test_deduction_guides() {
    using ComplexType = std::pair<int, std::string>;
    using ComplexTypeConst = std::pair<const int, std::string>;
    std::vector<ComplexType> v;
    auto l = { ComplexTypeConst(1, "one"), ComplexTypeConst(2, "two")};
    using custom_allocator_type = std::allocator<ComplexTypeConst>;

    // check TMap(InputIterator, InputIterator)
    TMap m0(v.begin(), v.end());
    static_assert(std::is_same<decltype(m0), TMap<int, std::string>>::value);

    // check TMap(InputIterator, InputIterator, size_t)
    TMap m1(v.begin(), v.end(), 1);
    static_assert(std::is_same<decltype(m1), TMap<int, std::string>>::value);

    // check TMap(InputIterator, InputIterator, size_t, Hasher)
    TMap m2(v.begin(), v.end(), 4, degenerate_hash<int>());
    static_assert(std::is_same<decltype(m2), TMap<int, std::string, degenerate_hash<int>>>::value);

    // check TMap(InputIterator, InputIterator, size_t, Hasher, Equality)
    TMap m3(v.begin(), v.end(), 4, degenerate_hash<int>(), std::less<int>());
    static_assert(std::is_same<decltype(m3), TMap<int, std::string, degenerate_hash<int>, std::less<int>>>::value);

    // check TMap(InputIterator, InputIterator, size_t, Hasher, Equality, Allocator)
    TMap m4(v.begin(), v.end(), 4, degenerate_hash<int>(), std::less<int>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(m4), TMap<int, std::string, degenerate_hash<int>,
        std::less<int>, custom_allocator_type>>::value);

    // check TMap(InputIterator, InputIterator, size_t, Allocator)
    TMap m5(v.begin(), v.end(), 5, custom_allocator_type{});
    static_assert(std::is_same<decltype(m5), TMap<int, std::string, std::hash<int>,
        std::equal_to<int>, custom_allocator_type>>::value);

    // check TMap(InputIterator, InputIterator, size_t, Hasher, Allocator)
    TMap m6(v.begin(), v.end(), 4, degenerate_hash<int>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(m6), TMap<int, std::string, degenerate_hash<int>,
        std::equal_to<int>, custom_allocator_type>>::value);

    // check TMap(std::initializer_list)
    TMap m7(l);
    static_assert(std::is_same<decltype(m7), TMap<int, std::string>>::value);

    // check TMap(std::initializer_list, size_t)
    TMap m8(l, 1);
    static_assert(std::is_same<decltype(m8), TMap<int, std::string>>::value);

    // check TMap(std::initializer_list, size_t, Hasher)
    TMap m9(l, 4, degenerate_hash<int>());
    static_assert(std::is_same<decltype(m9), TMap<int, std::string, degenerate_hash<int>>>::value);

    // check TMap(std::initializer_list, size_t, Hasher, Equality)
    TMap m10(l, 4, degenerate_hash<int>(), std::less<int>());
    static_assert(std::is_same<decltype(m10), TMap<int, std::string, degenerate_hash<int>, std::less<int>>>::value);

    // check TMap(std::initializer_list, size_t, Hasher, Equality, Allocator)
    TMap m11(l, 4, degenerate_hash<int>(), std::less<int>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(m11), TMap<int, std::string, degenerate_hash<int>,
        std::less<int>, custom_allocator_type>>::value);

    // check TMap(std::initializer_list, size_t, Allocator)
    TMap m12(l, 4, custom_allocator_type{});
    static_assert(std::is_same<decltype(m12), TMap<int, std::string, std::hash<int>,
        std::equal_to<int>, custom_allocator_type>>::value);

    // check TMap(std::initializer_list, size_t, Hasher, Allocator)
    TMap m13(l, 4, degenerate_hash<int>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(m13), TMap<int, std::string, degenerate_hash<int>,
        std::equal_to<int>, custom_allocator_type>>::value);

    // check TMap(TMap &)
    TMap m14(m1);
    static_assert(std::is_same<decltype(m14), decltype(m1)>::value);

    // check TMap(TMap &, Allocator)
    // TODO: investigate why no implicit deduction guides generated for this ctor
    TMap m15(m5, custom_allocator_type{});
    static_assert(std::is_same<decltype(m15), decltype(m5)>::value);

    // check TMap(TMap &&)
    TMap m16(std::move(m1));
    static_assert(std::is_same<decltype(m16), decltype(m1)>::value);

    // check TMap(TMap &&, Allocator)
    // TODO: investigate why no implicit deduction guides generated for this ctor
    TMap m17(std::move(m5), custom_allocator_type{});
    static_assert(std::is_same<decltype(m17), decltype(m5)>::value);
}
#endif

void test_heterogeneous_functions() {
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_map, int, int>();
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_multimap, int, int>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_map, std::string, std::string>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_multimap, std::string, std::string>();
}

struct CumapTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_unordered_map<T, T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct CumapTraits

struct CumultimapTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_unordered_multimap<T, T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    using init_iterator_type = move_support_tests::FooPairIterator;
}; // struct CumultimapTraits

//! Testing concurrent_unordered_map member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_map member types") {
    test_member_types<oneapi::tbb::concurrent_unordered_map>();
}

//! Testing requirements of concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_map requirements") {
    test_basic<map_type>();
}

//! Testing multithreading support in concurrent_unordered_map
//! \brief \ref requirement
TEST_CASE("concurrent_unordered_map multithreading support") {
    test_concurrent<map_type>();
}

//! Testing move constructors and assignment operator in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_map move semantics support") {
    test_rvalue_ref_support<CumapTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_unordered_map") {
    test_initializer_list_support<map_type>({{1, 1}, {2, 2}, {3, 3}, {4, 4}});
}

//! Testing node handling in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_unordered_map") {
    node_handling_tests::test_node_handling_support<map_type>();
}

//! Testing std::allocator_traits support in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_unordered_map") {
    test_allocator_traits_support<CumapTraits>();
}

//! Testing heterogeneous overloads in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_unordered_map") {
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_map, int, int>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_map, std::string, std::string>();
}

//! Testing insert overloads with generic pair in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("insertion by generic pair in concurrent_unordered_map") {
    test_insert_by_generic_pair<oneapi::tbb::concurrent_unordered_map>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_unordered_map") {
    test_deduction_guides<oneapi::tbb::concurrent_unordered_map>();
}
#endif

//! Testing comparisons in concurrent_unordered_map
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_map comparisons") {
    test_map_comparisons<oneapi::tbb::concurrent_unordered_map>();
}

//! Testing concurrent_unordered_multimap member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multimap member types") {
    test_member_types<oneapi::tbb::concurrent_unordered_multimap>();
}

//! Testing requirements of concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multimap requirements") {
    test_basic<multimap_type>();
}

//! Testing multithreading support in concurrent_unordered_multimap
//! \brief \ref requirement
TEST_CASE("concurrent_unordered_multimap multithreading support") {
    test_concurrent<multimap_type>();
}

//! Testing move constructors and assignment operator in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multimap move semantics support") {
    test_rvalue_ref_support<CumultimapTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_unordered_multimap") {
    test_initializer_list_support<multimap_type>({{1, 1}, {2, 2}, {3, 3}, {4, 4}, {4, 40}});
}

//! Testing node handling support in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_unordered_multimap") {
    node_handling_tests::test_node_handling_support<multimap_type>();
}

//! Testing std::allocator_traits support in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_unordered_multimap") {
    test_allocator_traits_support<CumultimapTraits>();
}

//! Testing heterogeneous overloads in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_unordered_multimap") {
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_multimap, int, int>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_multimap, std::string, std::string>();
}

//! Testing insert overloads with generic pair in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("insertion by generic pair in concurrent_unordered_multimap") {
    test_insert_by_generic_pair<oneapi::tbb::concurrent_unordered_multimap>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_unordered_multimap") {
    test_deduction_guides<oneapi::tbb::concurrent_unordered_multimap>();
}
#endif

//! Testing comparisons in concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multimap comparisons") {
    test_map_comparisons<oneapi::tbb::concurrent_unordered_multimap>();
}

//! Testing of merge operations in concurrent_unordered_map and concurrent_unordered_multimap
//! \brief \ref interface \ref requirement
TEST_CASE("merge operations") {
    node_handling_tests::test_merge<map_type, multimap_type>(1000);
}
