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


#include "oneapi/tbb/concurrent_unordered_set.h"
#include <common/test.h>
#include <common/utils.h>
#include <common/concurrent_unordered_common.h>
#include <memory>
#include <type_traits>

//! \file conformance_concurrent_unordered_set.cpp
//! \brief Test for [containers.concurrent_unordered_set containers.concurrent_unordered_multiset] specifications

template <typename... Args>
struct AllowMultimapping<oneapi::tbb::concurrent_unordered_multiset<Args...>> : std::true_type {};

template <typename Key>
using Allocator = LocalCountingAllocator<std::allocator<Key>>;

using set_type = oneapi::tbb::concurrent_unordered_set<int, std::hash<int>, std::equal_to<int>, Allocator<int>>;
using multiset_type = oneapi::tbb::concurrent_unordered_multiset<int, std::hash<int>, std::equal_to<int>, Allocator<int>>;

template <template <typename...> class ContainerType>
void test_member_types() {
    using default_container_type = ContainerType<int>;
    static_assert(std::is_same<typename default_container_type::hasher, std::hash<int>>::value,
                  "Incorrect default template hasher");
    static_assert(std::is_same<typename default_container_type::key_equal, std::equal_to<int>>::value,
                  "Incorrect default template key equality");
    static_assert(std::is_same<typename default_container_type::allocator_type, oneapi::tbb::tbb_allocator<int>>::value,
                  "Incorrect default template allocator");

    auto test_hasher = [](const int&)->std::size_t { return 0; };
    auto test_equality = [](const int&, const int&)->bool { return true; };
    using test_allocator_type = std::allocator<int>;

    using container_type = ContainerType<int, decltype(test_hasher), decltype(test_equality), test_allocator_type>;

    static_assert(std::is_same<typename container_type::key_type, int>::value,
                  "Incorrect container key_type member type");
    static_assert(std::is_same<typename container_type::value_type, int>::value,
                  "Incorrect container value_type member type");

    static_assert(std::is_unsigned<typename container_type::size_type>::value,
                  "Incorrect container size_type member type");
    static_assert(std::is_signed<typename container_type::difference_type>::value,
                  "Incorrect container difference_type member type");

    static_assert(std::is_same<typename container_type::hasher, decltype(test_hasher)>::value,
                  "Incorrect container hasher member type");
    static_assert(std::is_same<typename container_type::key_equal, decltype(test_equality)>::value,
                  "Incorrect container key_equal member type");

    using transparent_container_type = ContainerType<int, hasher_with_transparent_key_equal,
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

struct CusetTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_unordered_set<T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct CusetTraits

struct CumultisetTraits : UnorderedMoveTraitsBase {
    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_unordered_multiset<T, std::hash<T>, std::equal_to<T>, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
}; // struct CumultisetTraits

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <template <typename ...> typename TSet>
void test_deduction_guides() {
    using ComplexType = const std::string *;
    std::vector<ComplexType> v;
    std::string s = "s";
    auto l = { ComplexType(&s), ComplexType(&s)};
    using custom_allocator_type = std::allocator<ComplexType>;

    // check TSet(InputIterator,InputIterator)
    TSet s1(v.begin(), v.end());
    static_assert(std::is_same<decltype(s1), TSet<ComplexType>>::value);

    // check TSet(InputIterator,InputIterator, size_t, Hasher)
    TSet s2(v.begin(), v.end(), 5, degenerate_hash<ComplexType>());
    static_assert(std::is_same<decltype(s2), TSet<ComplexType, degenerate_hash<ComplexType>>>::value);

    // check TSet(InputIterator,InputIterator, size_t, Hasher, Equality)
    TSet s3(v.begin(), v.end(), 5, degenerate_hash<ComplexType>(), std::less<ComplexType>());
    static_assert(std::is_same<decltype(s3), TSet<ComplexType, degenerate_hash<ComplexType>,
        std::less<ComplexType>>>::value);

    // check TSet(InputIterator,InputIterator, size_t, Hasher, Equality, Allocator)
    TSet s4(v.begin(), v.end(), 5, degenerate_hash<ComplexType>(), std::less<ComplexType>(),
            custom_allocator_type{});
    static_assert(std::is_same<decltype(s4), TSet<ComplexType, degenerate_hash<ComplexType>,
        std::less<ComplexType>, custom_allocator_type>>::value);

    // check TSet(InputIterator,InputIterator, size_t, Allocator)
    TSet s5(v.begin(), v.end(), 5, custom_allocator_type{});
    static_assert(std::is_same<decltype(s5), TSet<ComplexType, std::hash<ComplexType>,
        std::equal_to<ComplexType>, custom_allocator_type>>::value);

    // check TSet(InputIterator,InputIterator, size_t, Hasher, Allocator)
    TSet s6(v.begin(), v.end(), 5, degenerate_hash<ComplexType>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(s6), TSet<ComplexType, degenerate_hash<ComplexType>,
        std::equal_to<ComplexType>, custom_allocator_type>>::value);

    // check TSet(std::initializer_list)
    TSet s7(l);
    static_assert(std::is_same<decltype(s7), TSet<ComplexType>>::value);

    // check TSet(std::initializer_list, size_t, Hasher)
    TSet s8(l, 5, degenerate_hash<ComplexType>());
    static_assert(std::is_same<decltype(s8), TSet<ComplexType, degenerate_hash<ComplexType>>>::value);

    // check TSet(std::initializer_list, size_t, Hasher, Equality)
    TSet s9(l, 5, degenerate_hash<ComplexType>(), std::less<ComplexType>());
    static_assert(std::is_same<decltype(s9), TSet<ComplexType, degenerate_hash<ComplexType>,
        std::less<ComplexType>>>::value);

    // check TSet(std::initializer_list, size_t, Hasher, Equality, Allocator)
    TSet s10(l, 5, degenerate_hash<ComplexType>(), std::less<ComplexType>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(s10), TSet<ComplexType, degenerate_hash<ComplexType>,
        std::less<ComplexType>, custom_allocator_type>>::value);

    // check TSet(std::initializer_list, size_t, Allocator)
    TSet s11(l, 5, custom_allocator_type{});
    static_assert(std::is_same<decltype(s11), TSet<ComplexType, std::hash<ComplexType>,
        std::equal_to<ComplexType>, custom_allocator_type>>::value);

    // check TSet(std::initializer_list, size_t, Hasher, Allocator)
    TSet s12(l, 5, std::hash<ComplexType>(), custom_allocator_type{});
    static_assert(std::is_same<decltype(s12), TSet<ComplexType, std::hash<ComplexType>,
        std::equal_to<ComplexType>, custom_allocator_type>>::value);

    // check TSet(TSet &)
    TSet s13(s1);
    static_assert(std::is_same<decltype(s13), decltype(s1)>::value);

    // check TSet(TSet &, Allocator)
    TSet s14(s5, custom_allocator_type{});
    // TODO: investigate why no implicit deduction guides generated for this ctor
    static_assert(std::is_same<decltype(s14), decltype(s5)>::value);

    // check TSet(TSet &&)
    TSet s15(std::move(s1));
    static_assert(std::is_same<decltype(s15), decltype(s1)>::value);

    // check TSet(TSet &&, Allocator)
    TSet s16(std::move(s5), custom_allocator_type{});
    // TODO: investigate why no implicit deduction guides generated for this ctor
    static_assert(std::is_same<decltype(s16), decltype(s5)>::value);
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing concurrent_unordered_set member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_set member types") {
    test_member_types<oneapi::tbb::concurrent_unordered_set>();
}

//! Testing requirements of concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_set requirements") {
    test_basic<set_type>();
}

//! Testing multithreading support in concurrent_unordered_set
//! \brief \ref requirement
TEST_CASE("concurrent_unordered_set multithreading support") {
    test_concurrent<set_type>();
}

//! Testing move constructors and assignment operator in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_set move semantics support") {
    test_rvalue_ref_support<CusetTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_unordered_set") {
    test_initializer_list_support<set_type>({1, 2, 3, 4, 5});
}

//! Testing node handling in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_unordered_set") {
    node_handling_tests::test_node_handling_support<set_type>();
}

//! Testing std::allocator_traits support in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_unordered_set") {
    test_allocator_traits_support<CusetTraits>();
}

//! Testing heterogeneous overloads in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_unordered_set") {
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_set, int>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_set, std::string>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_unordered_set") {
    test_deduction_guides<oneapi::tbb::concurrent_unordered_set>();
}
#endif

//! Testing comparisons in concurrent_unordered_set
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_set comparisons") {
    test_set_comparisons<oneapi::tbb::concurrent_unordered_set>();
}

//! Testing concurrent_unordered_multiset member types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multiset member types") {
    test_member_types<oneapi::tbb::concurrent_unordered_multiset>();
}

//! Testing requirements of concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multiset requirements") {
    test_basic<multiset_type>();
}

//! Testing move constructors and assignment operator in concurrent_unordered_multiset
//! \brief \ref requirement
TEST_CASE("concurrent_unordered_multiset multithreading support") {
    test_concurrent<multiset_type>();
}

//! Testing move constructors and assignment operator in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multiset move semantics support") {
    test_rvalue_ref_support<CumultisetTraits>();
}

//! Testing std::initializer_list constructors and modifiers in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_unordered_multiset") {
    test_initializer_list_support<multiset_type>({1, 2, 3, 4, 5, 5});
}

//! Testing node handling support in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("node handling support in concurrent_unordered_multiset") {
    node_handling_tests::test_node_handling_support<multiset_type>();
}

//! Testing std::allocator_traits support in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_unordered_multiset") {
    test_allocator_traits_support<CumultisetTraits>();
}

//! Testing heterogeneous overloads in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("heterogeneous overloads in concurrent_unordered_multiset") {
    check_heterogeneous_functions_key_int<oneapi::tbb::concurrent_unordered_multiset, int>();
    check_heterogeneous_functions_key_string<oneapi::tbb::concurrent_unordered_multiset, std::string>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_unordered_multiset") {
    test_deduction_guides<oneapi::tbb::concurrent_unordered_multiset>();
}
#endif

//! Testing comparisons in concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_unordered_multiset comparisons") {
    test_set_comparisons<oneapi::tbb::concurrent_unordered_multiset>();
}

//! Testing of merge operation in concurrent_unordered_set and concurrent_unordered_multiset
//! \brief \ref interface \ref requirement
TEST_CASE("merge operations") {
    node_handling_tests::test_merge<set_type, multiset_type>(1000);
}
