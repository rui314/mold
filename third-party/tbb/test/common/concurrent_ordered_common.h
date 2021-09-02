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

#ifndef __TBB_test_common_concurrent_ordered_common_H
#define __TBB_test_common_concurrent_ordered_common_H

#define __TBB_TEST_CPP20_COMPARISONS __TBB_CPP20_COMPARISONS_PRESENT && __TBB_CPP20_CONCEPTS_PRESENT

#include "config.h"

#include "common/concurrent_associative_common.h"
#include "test_comparisons.h"

template<typename MyTable>
inline void CheckContainerAllocator(MyTable &table, size_t expected_allocs, size_t expected_frees, bool exact) {
    typename MyTable::allocator_type a = table.get_allocator();
    CheckAllocator<MyTable>(a, expected_allocs, expected_frees, exact);
}

template<typename Container>
inline void CheckNoAllocations(Container&) {
    // TODO: enable
    // CheckContainerAllocator(cont, 0, 0, false);
}

template <typename Container>
struct OrderChecker {
    typename Container::value_compare& val_comp;
    typename Container::key_compare& key_comp;

    OrderChecker(typename Container::value_compare& v_comp, typename Container::key_compare& k_comp)
        : val_comp(v_comp), key_comp(k_comp) {}

    bool operator()(const typename Container::value_type& lhs, const typename Container::value_type& rhs) {
        if (AllowMultimapping<Container>::value)
            // We need to use not greater comparator for multicontainer
            return !val_comp(rhs, lhs) && !key_comp(Value<Container>::key(rhs), Value<Container>::key(lhs));
        return val_comp(lhs, rhs) && key_comp(Value<Container>::key(lhs), Value<Container>::key(rhs));
    }
}; // struct OrderChecker

template <typename Container>
void check_container_order( const Container& cont ) {
    if (!cont.empty()) {
        typename Container::key_compare key_comp = cont.key_comp();
        typename Container::value_compare value_comp = cont.value_comp();
        OrderChecker<Container> check_order(value_comp, key_comp);

        for (auto it = cont.begin(); std::next(it) != cont.end();) {
            auto pr_it = it++;
            REQUIRE_MESSAGE(check_order(*pr_it, *it), "The order of the elements is broken");
        }
    }
}

template <typename Container>
void test_ordered_methods() {
    Container cont;
    const Container& ccont = cont;

    int r, random_threshold = 10, uncontained_key = random_threshold / 2;
    for (int i = 0; i < 100; ++i) {
        r = std::rand() % random_threshold;
        if (r != uncontained_key) {
            cont.insert(Value<Container>::make(r));
        }
    }

    check_container_order(cont);

    typename Container::value_compare val_comp = cont.value_comp();
    typename Container::iterator l_bound_check, u_bound_check;
    for (int key = -1; key < random_threshold + 1; ++key) {
        auto eq_range = cont.equal_range(key);
        // Check equal_range content
        for (auto it = eq_range.first; it != eq_range.second; ++it)
            REQUIRE_MESSAGE(*it == Value<Container>::make(key), "equal_range contains wrong value");

        // Manual search of upper and lower bounds
        l_bound_check = cont.end();
        u_bound_check = cont.end();
        for (auto it = cont.begin(); it != cont.end(); ++it){
            if (!val_comp(*it, Value<Container>::make(key)) && l_bound_check == cont.end()) {
                l_bound_check = it;
            }
            if (val_comp(Value<Container>::make(key), *it) && u_bound_check == cont.end()) {
                u_bound_check = it;
                break;
            }
        }

        typename Container::range_type cont_range = cont.range();
        typename Container::const_range_type ccont_range = ccont.range();
        REQUIRE_MESSAGE(cont_range.size() == ccont_range.size(), "Incorrect ordered container range size");
        REQUIRE_MESSAGE(cont_range.size() == cont.size(), "Incorrect ordered container range size");

        typename Container::iterator l_bound = cont.lower_bound(key);
        typename Container::iterator u_bound = cont.upper_bound(key);

        REQUIRE_MESSAGE(l_bound == l_bound_check, "lower_bound() returned wrong iterator");
        REQUIRE_MESSAGE(u_bound == u_bound_check, "upper_bound() returned wrong iterator");

        using const_iterator = typename Container::const_iterator;
        const_iterator cl_bound = ccont.lower_bound(key);
        const_iterator cu_bound = ccont.upper_bound(key);

        REQUIRE_MESSAGE(cl_bound == const_iterator(l_bound), "lower_bound() const returned wrong iterator");
        REQUIRE_MESSAGE(cu_bound == const_iterator(u_bound), "upper_bound() const returned wrong iterator");

        REQUIRE((l_bound == eq_range.first && u_bound == eq_range.second));
    }
}

template <typename Container, typename CheckElementState = std::false_type>
void test_basic() {
    test_basic_common<Container, CheckElementState>();
    test_ordered_methods<Container>();
}

template <typename Container>
void test_concurrent_order() {
    // TODO: MinThread - MaxThread loop
    auto num_threads = utils::get_platform_max_threads();
    Container cont;
    int items = 1000;
    utils::NativeParallelFor(num_threads, [&](std::size_t index) {
        int step = index % 4 + 1;
        bool reverse = (step % 2 == 0);
        if (reverse) {
            for (int i = 0; i < items; i += step) {
                cont.insert(Value<Container>::make(i));
            }
        } else {
            for (int i = items; i > 0; i -= step) {
                cont.insert(Value<Container>::make(i));
            }
        }
    });

    check_container_order(cont);
}

template <typename Container>
void test_concurrent(bool asymptotic = false) {
    test_concurrent_common<Container>(asymptotic);
    test_concurrent_order<Container>();
}

struct OrderedMoveTraitsBase {
    static constexpr std::size_t expected_number_of_items_to_allocate_for_steal_move = 584; // TODO: remove allocation of dummy_node

    template <typename OrderedType, typename Iterator>
    static OrderedType& construct_container( typename std::aligned_storage<sizeof(OrderedType)>::type& storage,
                                             Iterator begin, Iterator end )
    {
        OrderedType* ptr = reinterpret_cast<OrderedType*>(&storage);
        new (ptr) OrderedType(begin, end);
        return *ptr;
    }

    template <typename OrderedType, typename Iterator, typename Allocator>
    static OrderedType& construct_container( typename std::aligned_storage<sizeof(OrderedType)>::type& storage,
                                             Iterator begin, Iterator end, const Allocator& alloc )
    {
        OrderedType* ptr = reinterpret_cast<OrderedType*>(&storage);
        new (ptr) OrderedType(begin, end, typename OrderedType::key_compare(), alloc);
        return *ptr;
    }

    template <typename OrderedType, typename Iterator>
    static bool equal( const OrderedType& c, Iterator begin, Iterator end ) {
        bool equal_sizes = std::size_t(std::distance(begin, end)) == c.size();
        if (!equal_sizes) return false;
        for (Iterator it = begin; it != end; ++it) {
            if (!c.contains(Value<OrderedType>::key((*it)))) return false;
        }
        return true;
    }
};

namespace std {
template <>
struct less<std::weak_ptr<int>> {
    bool operator()( const std::weak_ptr<int>& lhs, const std::weak_ptr<int>& rhs ) const {
        return *lhs.lock() < *rhs.lock();
    }
};

template <>
struct less<const std::weak_ptr<int>> : less<std::weak_ptr<int>> {};
}

template <bool DefCtorPresent, typename Table>
void Examine(Table c, const std::list<typename Table::value_type>& lst) {
    CommonExamine<DefCtorPresent>(c, lst);
}

template <bool DefCtorPresent, typename Table>
void TypeTester( const std::list<typename Table::value_type>& lst ) {
    REQUIRE_MESSAGE(lst.size() >= 5, "Array should have at least 5 elements");
    REQUIRE_MESSAGE(lst.size() <= 100, "The test has O(n^2) complexity so a big number of elements can lead long execution time");

    // Construct an empty table
    Table c1;
    c1.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c1, lst);

    typename Table::key_compare compare;
    typename Table::allocator_type allocator;

    auto it = lst.begin();
    auto init = {*it++, *it++, *it++};

    // Constructor from an std::initializer_list
    Table c2(init);
    c2.insert(it, lst.end());
    Examine<DefCtorPresent>(c2, lst);

    // Constructor from an std::initializer_list, default comparator and non-default allocator
    Table c2_alloc(init, allocator);
    c2_alloc.insert(it, lst.end());
    Examine<DefCtorPresent>(c2_alloc, lst);

    // Constructor from an std::initializer_list, non-default comparator and allocator
    Table c2_comp_alloc(init, compare, allocator);
    c2_comp_alloc.insert(it, lst.end());
    Examine<DefCtorPresent>(c2_comp_alloc, lst);

    // Copying constructor
    Table c3(c1);
    Examine<DefCtorPresent>(c3, lst);

    // Copying constructor with the allocator
    Table c3_alloc(c1, allocator);
    Examine<DefCtorPresent>(c3_alloc, lst);

    // Constructor with non-default compare
    Table c4(compare);
    c4.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c4, lst);

    // Constructor with non-default allocator
    Table c5(allocator);
    c5.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c5, lst);

    // Constructor with non-default compare and non-default allocator
    Table c6(compare, allocator);
    c6.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c6, lst);

    // Constructor from an iteration range
    Table c7(c1.begin(), c1.end());
    Examine<DefCtorPresent>(c7, lst);

    // Constructor from an iteration range, default compare and non-default allocator
    Table c8(c1.begin(), c1.end(), allocator);
    Examine<DefCtorPresent>(c8, lst);

    // Constructor from an iteration range, non-default compare and non-default allocator
    Table c9(c1.begin(), c1.end(), compare, allocator);
    Examine<DefCtorPresent>(c9, lst);
}

struct TransparentLess {
    template <typename T, typename U>
    auto operator()( T&& lhs, U&& rhs ) const
    -> decltype(std::forward<T>(lhs) < std::forward<U>(rhs)) {
        return lhs < rhs;
    }

    using is_transparent = void;
};

template <template <typename...> class Container, typename... Args>
void check_heterogeneous_functions_key_int() {
    check_heterogeneous_functions_key_int_impl<Container<Args..., TransparentLess>>();
}

template <template <typename...> class Container, typename... Args>
void check_heterogeneous_functions_key_string() {
    check_heterogeneous_functions_key_string_impl<Container<Args..., TransparentLess>>();
}

template <typename Container>
void check_heterogeneous_bound_functions() {
    static_assert(std::is_same<typename Container::key_type, int>::value,
                  "incorrect key_type for heterogeneous bounds test");
    // Initialization
    Container c;
    const Container& cc = c;

    int size = 10;
    for (int i = 0; i < size; ++i) {
        c.insert(Value<Container>::make(i));
    }
    // Insert first duplicated element for multicontainers
    if (AllowMultimapping<Container>::value) {
        c.insert(Value<Container>::make(0));
    }

    // Upper and lower bound testing
    for (int i = 0; i < size; ++i) {
        int_key k(i);
        int key = i;

        REQUIRE_MESSAGE(c.lower_bound(k) == c.lower_bound(key), "Incorrect heterogeneous lower_bound return value");
        REQUIRE_MESSAGE(c.upper_bound(k) == c.upper_bound(key), "Incorrect heterogeneous upper_bound return value");
        REQUIRE_MESSAGE(cc.lower_bound(k) == cc.lower_bound(key), "Incorrect const heterogeneous lower_bound return value");
        REQUIRE_MESSAGE(cc.upper_bound(k) == cc.upper_bound(key), "Incorrect const heterogeneous upper_bound return value");
    }
}

template <typename Container>
void test_comparisons_basic() {
    using comparisons_testing::testEqualityAndLessComparisons;
    Container c1, c2;
    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(c1, c2);

    c1.insert(Value<Container>::make(1));
    testEqualityAndLessComparisons</*ExpectEqual = */false, /*ExpectLess = */false>(c1, c2);

    c2.insert(Value<Container>::make(1));
    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(c1, c2);

    c2.insert(Value<Container>::make(2));
    testEqualityAndLessComparisons</*ExpectEqual = */false, /*ExpectLess = */true>(c1, c2);

    c1.clear();
    c2.clear();

    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(c1, c2);
}

template <typename TwoWayComparableContainerType>
void test_two_way_comparable_container() {
    TwoWayComparableContainerType c1, c2;
    c1.insert(Value<TwoWayComparableContainerType>::make(1));
    c2.insert(Value<TwoWayComparableContainerType>::make(1));
    comparisons_testing::TwoWayComparable::reset();
    REQUIRE_MESSAGE(!(c1 < c2), "Incorrect operator < result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(!(c1 > c2), "Incorrect operator > result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(c1 <= c2, "Incorrect operator <= result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(c1 >= c2, "Incorrect operator >= result");
    comparisons_testing::check_two_way_comparison();
}

#if __TBB_TEST_CPP20_COMPARISONS
template <typename ThreeWayComparableContainerType>
void test_three_way_comparable_container() {
    ThreeWayComparableContainerType c1, c2;
    c1.insert(Value<ThreeWayComparableContainerType>::make(1));
    c2.insert(Value<ThreeWayComparableContainerType>::make(1));
    comparisons_testing::ThreeWayComparable::reset();
    REQUIRE_MESSAGE(!(c1 <=> c2 < 0), "Incorrect operator<=> result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(!(c1 < c2), "Incorrect operator< result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(!(c1 > c2), "Incorrect operator> result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(c1 <= c2, "Incorrect operator>= result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(c1 >= c2, "Incorrect operator>= result");
    comparisons_testing::check_three_way_comparison();
}
#endif

template <template <typename...> class ContainerType>
void test_map_comparisons() {
    using integral_container = ContainerType<int, int>;
    using two_way_comparable_container = ContainerType<comparisons_testing::TwoWayComparable,
                                                       comparisons_testing::TwoWayComparable>;
    test_comparisons_basic<integral_container>();
    test_comparisons_basic<two_way_comparable_container>();
    test_two_way_comparable_container<two_way_comparable_container>();

#if __TBB_TEST_CPP20_COMPARISONS
    using two_way_less_only_container = ContainerType<comparisons_testing::LessComparableOnly,
                                                      comparisons_testing::LessComparableOnly>;

    using three_way_only_container = ContainerType<comparisons_testing::ThreeWayComparableOnly,
                                                   comparisons_testing::ThreeWayComparableOnly>;

    using three_way_comparable_container = ContainerType<comparisons_testing::ThreeWayComparable,
                                                         comparisons_testing::ThreeWayComparable>;

    test_comparisons_basic<two_way_less_only_container>();
    test_comparisons_basic<three_way_only_container>();
    test_comparisons_basic<three_way_comparable_container>();
    test_three_way_comparable_container<three_way_comparable_container>();
#endif // __TBB_TEST_CPP20_COMPARISONS
}

template <template <typename...> class ContainerType>
void test_set_comparisons() {
    using integral_container = ContainerType<int>;
    using two_way_comparable_container = ContainerType<comparisons_testing::TwoWayComparable>;

    test_comparisons_basic<integral_container>();
    test_comparisons_basic<two_way_comparable_container>();
    test_two_way_comparable_container<two_way_comparable_container>();

#if __TBB_TEST_CPP20_COMPARISONS
    using two_way_less_only_container = ContainerType<comparisons_testing::LessComparableOnly>;
    using three_way_only_container = ContainerType<comparisons_testing::ThreeWayComparableOnly>;
    using three_way_comparable_container = ContainerType<comparisons_testing::ThreeWayComparable>;

    test_comparisons_basic<two_way_less_only_container>();
    test_comparisons_basic<three_way_only_container>();
    test_comparisons_basic<three_way_comparable_container>();
    test_three_way_comparable_container<three_way_comparable_container>();
#endif // __TBB_TEST_CPP20_COMPARISONS
}

#endif // __TBB_test_common_concurrent_ordered_common_H
