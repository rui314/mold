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

#ifndef __TBB_test_common_concurrent_unordered_common
#define __TBB_test_common_concurrent_unordered_common

#define __TBB_UNORDERED_TEST 1

#include "test.h"
#include <memory>
#include "concurrent_associative_common.h"
#include "test_comparisons.h"

template<typename MyTable>
inline void CheckContainerAllocator(MyTable &table, size_t expected_allocs, size_t expected_frees, bool exact) {
    typename MyTable::allocator_type a = table.get_allocator();
    REQUIRE( a.items_allocated == a.allocations);
    REQUIRE( a.items_freed == a.frees);
    REQUIRE( a.items_allocated == a.items_freed );
    CheckAllocator<MyTable>(a, expected_allocs, expected_frees, exact);
}

template<typename Container>
inline void CheckNoAllocations(Container &cont){
    CheckContainerAllocator(cont, 0, 0, false);
}

template<typename T>
struct degenerate_hash {
    size_t operator()(const T& /*a*/) const {
        return 1;
    }
};

template <typename T>
void test_unordered_methods(){
    T cont;
    cont.insert(Value<T>::make(1));
    cont.insert(Value<T>::make(2));
    // unordered_specific
    // void rehash(size_type n);
    cont.rehash(16);

    // float load_factor() const;
    // float max_load_factor() const;
    REQUIRE_MESSAGE(cont.load_factor() <= cont.max_load_factor(), "Load factor is invalid");

    // void max_load_factor(float z);
    cont.max_load_factor(16.0f);
    REQUIRE_MESSAGE(cont.max_load_factor() == 16.0f, "Max load factor has not been changed properly");

    // hasher hash_function() const;
    cont.hash_function();

    // key_equal key_eq() const;
    cont.key_eq();

    cont.clear();
    CheckNoAllocations(cont);
    for (int i = 0; i < 256; i++)
    {
        std::pair<typename T::iterator, bool> ins3 = cont.insert(Value<T>::make(i));
        REQUIRE_MESSAGE((ins3.second == true && Value<T>::get(*(ins3.first)) == i), "Element 1 has not been inserted properly");
    }
    REQUIRE_MESSAGE(cont.size() == 256, "Wrong number of elements have been inserted");
    // size_type unsafe_bucket_count() const;
    REQUIRE_MESSAGE(cont.unsafe_bucket_count() == 16, "Wrong number of buckets");

    // size_type unsafe_max_bucket_count() const;
    //REQUIRE_MESSAGE(cont.unsafe_max_bucket_count() > 65536, "Wrong max number of buckets");

    for (unsigned int i = 0; i < 256; i++)
    {
        typename T::size_type buck = cont.unsafe_bucket(i);

        // size_type unsafe_bucket(const key_type& k) const;
        REQUIRE_MESSAGE(buck < 16, "Wrong bucket mapping");
    }

    typename T::size_type bucketSizeSum = 0;
    typename T::size_type iteratorSizeSum = 0;

    for (unsigned int i = 0; i < 16; i++)
    {
        bucketSizeSum += cont.unsafe_bucket_size(i);
        for (typename T::iterator bit = cont.unsafe_begin(i); bit != cont.unsafe_end(i); bit++) iteratorSizeSum++;
    }
    REQUIRE_MESSAGE(bucketSizeSum == 256, "sum of bucket counts incorrect");
    REQUIRE_MESSAGE(iteratorSizeSum == 256, "sum of iterator counts incorrect");
}

template<typename Container, typename CheckElementState = std::false_type>
void test_basic(){
    test_basic_common<Container, CheckElementState>();
    test_unordered_methods<Container>();
}

template <typename Container>
void test_concurrent( bool asymptotic = false ) {
    test_concurrent_common<Container>(asymptotic);
}

struct UnorderedMoveTraitsBase {
    static constexpr std::size_t expected_number_of_items_to_allocate_for_steal_move = 3; // TODO: check

    template <typename UnorderedType, typename Iterator>
    static UnorderedType& construct_container( typename std::aligned_storage<sizeof(UnorderedType)>::type& storage,
                                               Iterator begin, Iterator end )
    {
        UnorderedType* ptr = reinterpret_cast<UnorderedType*>(&storage);
        new (ptr) UnorderedType(begin, end);
        return *ptr;
    }

    template <typename UnorderedType, typename Iterator, typename Allocator>
    static UnorderedType& construct_container( typename std::aligned_storage<sizeof(UnorderedType)>::type& storage,
                                                Iterator begin, Iterator end, const Allocator& alloc )
    {
        UnorderedType* ptr = reinterpret_cast<UnorderedType*>(&storage);
        new (ptr) UnorderedType(begin, end, /*bucket_count = */4, alloc);
        return *ptr;
    }

    template <typename UnorderedType, typename Iterator>
    static bool equal( const UnorderedType& c, Iterator begin, Iterator end ) {
        if (std::size_t(std::distance(begin, end)) != c.size()) {
            return false;
        }

        for (Iterator it = begin; it != end; ++it) {
            if (!c.contains(Value<UnorderedType>::key(*it))) {
                return false;
            }
        }
        return true;
    }
}; // struct UnorderedMoveTraitsBase

template <bool DefCtorPresent, typename Table>
void CustomExamine( Table c, const std::list<typename Table::value_type>& lst ) {
    using size_type = typename Table::size_type;
    const Table constC = c;

    const size_type bucket_count = c.unsafe_bucket_count();
    REQUIRE(c.unsafe_max_bucket_count() >= bucket_count);

    size_type counter = 0;
    for (size_type i = 0; i < bucket_count; ++i) {
        const size_type size = c.unsafe_bucket_size(i);
        using diff_type = typename Table::difference_type;

        REQUIRE(std::distance(c.unsafe_begin(i), c.unsafe_end(i)) == diff_type(size));
        REQUIRE(std::distance(c.unsafe_cbegin(i), c.unsafe_cend(i)) == diff_type(size));
        REQUIRE(std::distance(constC.unsafe_begin(i), constC.unsafe_end(i)) == diff_type(size));
        REQUIRE(std::distance(constC.unsafe_cbegin(i), constC.unsafe_cend(i)) == diff_type(size));
        counter += size;
    }

    REQUIRE(counter == lst.size());

    for (auto it = lst.begin(); it != lst.end();) {
        const size_type index = c.unsafe_bucket(Value<Table>::key(*it));
        auto prev_it = it++;
        REQUIRE(std::search(c.unsafe_begin(index), c.unsafe_end(index), prev_it, it, utils::IsEqual()) != c.unsafe_end(index));
    }

    c.rehash(2*bucket_count);
    REQUIRE(c.unsafe_bucket_count() > bucket_count);

    auto count = 2 * c.max_load_factor() * c.unsafe_bucket_count();
    c.reserve(size_type(count));
    REQUIRE(c.max_load_factor() * c.unsafe_bucket_count() >= count);

    REQUIRE(c.load_factor() <= c.max_load_factor());
    c.max_load_factor(1.0f);
    c.hash_function();
    c.key_eq();
}

template <bool DefCtorPresent, typename Table>
void Examine( Table c, const std::list<typename Table::value_type>& lst ) {
    CommonExamine<DefCtorPresent>(c, lst);
    CustomExamine<DefCtorPresent>(c, lst);
}

// Necessary to avoid warnings about explicit copy assignment to itself
template <typename T>
T& self( T& obj ) {
    return obj;
}

template <bool DefCtorPresent, typename Table>
void TypeTester( const std::list<typename Table::value_type>& lst ) {
    REQUIRE_MESSAGE(lst.size() >= 5, "Array should have at least 5 elements");
    REQUIRE_MESSAGE(lst.size() <= 100, "The test hash O(n^2) complexity so a big number of elements can lead long execution time");

    Table c1;
    c1.insert(lst.begin(), lst.end());

    Examine<DefCtorPresent>(c1, lst);

    typename Table::size_type initial_bucket_number = 8;
    typename Table::allocator_type allocator;
    typename Table::hasher hasher;

    auto it = lst.begin();
    Table c2({*it++, *it++, *it++});
    c2.insert(it, lst.end());
    Examine<DefCtorPresent>(c2, lst);

    it = lst.begin();
    // Constructor from an std::initializer_list, default hasher and key_equal and non-default allocator
    Table c2_alloc({*it++, *it++, *it++}, initial_bucket_number, allocator);
    c2_alloc.insert(it, lst.end());
    Examine<DefCtorPresent>(c2_alloc, lst);

    it = lst.begin();
    // Constructor from an std::initializer_list, default key_equal and non-default hasher and allocator
    Table c2_hash_alloc({*it++, *it++, *it++}, initial_bucket_number, hasher, allocator);
    c2_hash_alloc.insert(it, lst.end());
    Examine<DefCtorPresent>(c2_hash_alloc, lst);

    // Copy ctor
    Table c3(c1);
    Examine<DefCtorPresent>(c3, lst);

    // Copy ctor with the allocator
    Table c3_alloc(c1, allocator);
    Examine<DefCtorPresent>(c3_alloc, lst);

    // Construct an empty table with n preallocated buckets
    Table c4(lst.size());
    c4.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c4, lst);

    // Construct an empty table with n preallocated buckets, default hasher and key_equal and non-default allocator
    Table c4_alloc(lst.size(), allocator);
    c4_alloc.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c4_alloc, lst);

    // Construct an empty table with n preallocated buckets, default key_equal and non-default hasher and allocator
    Table c4_hash_alloc(lst.size(), hasher, allocator);
    c4_hash_alloc.insert(lst.begin(), lst.end());
    Examine<DefCtorPresent>(c4_hash_alloc, lst);

    // Construction from the iteration range
    Table c5(c1.begin(), c1.end());
    Examine<DefCtorPresent>(c5, lst);

    // Construction from the iteration range, default hasher and key_equal and non-default allocator
    Table c5_alloc(c1.begin(), c2.end(), initial_bucket_number, allocator);
    Examine<DefCtorPresent>(c5_alloc, lst);

    // Construction from the iteration range, default key_equal and non-default hasher and allocator
    Table c5_hash_alloc(c1.begin(), c2.end(), initial_bucket_number, hasher, allocator);
    Examine<DefCtorPresent>(c5_hash_alloc, lst);

    // Copy assignment
    Table c6;
    c6 = c1;
    Examine<DefCtorPresent>(c6, lst);

    // Copy assignment to itself
    c6 = self(c6);
    Examine<DefCtorPresent>(c6, lst);

    // Move assignment
    Table c7;
    c7 = std::move(c6);
    Examine<DefCtorPresent>(c7, lst);

    // Move assignment to itself
    c7 = std::move(self(c7));
    Examine<DefCtorPresent>(c7, lst);

    // Assignment to the std::initializer_list
    Table c8;
    it = lst.begin();
    c8 = {*it++, *it++, *it++};
    c8.insert(it, lst.end());
    Examine<DefCtorPresent>(c8, lst);
}

struct transparent_key_equality {
template <typename T>
    bool operator()(const T&, const T&) const {
        return true;
    }
    using is_transparent = void;
};

struct hasher_with_transparent_key_equal {
template <typename T>
    std::size_t operator()(const T&) {
        return 0;
    }
    using transparent_key_equal = transparent_key_equality;
};

template <template <typename...> class Container, typename... Args>
void check_heterogeneous_functions_key_int() {
    check_heterogeneous_functions_key_int_impl<Container<Args..., hash_with_transparent_key_equal>>();
}

template <template <typename...> class Container, typename... Args>
void check_heterogeneous_functions_key_string() {
    check_heterogeneous_functions_key_string_impl<Container<Args..., hash_with_transparent_key_equal>>();
}

template <typename Container>
void test_comparisons_basic() {
    using comparisons_testing::testEqualityComparisons;
    Container c1, c2;
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);

    c1.insert(Value<Container>::make(1));
    testEqualityComparisons</*ExpectEqual = */false>(c1, c2);

    c2.insert(Value<Container>::make(1));
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);

    c2.insert(Value<Container>::make(2));
    testEqualityComparisons</*ExpectEqual = */false>(c1, c2);

    c1.clear();
    c2.clear();
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);
}

template <typename TwoWayComparableContainerType>
void test_two_way_comparable_container() {
    TwoWayComparableContainerType c1, c2;
    c1.insert(Value<TwoWayComparableContainerType>::make(1));
    c2.insert(Value<TwoWayComparableContainerType>::make(1));
    comparisons_testing::TwoWayComparable::reset();
    REQUIRE_MESSAGE(c1 == c2, "Incorrect operator == result");
    comparisons_testing::check_equality_comparison();
    REQUIRE_MESSAGE(!(c1 != c2), "Incorrect operator != result");
    comparisons_testing::check_equality_comparison();
}

template <template <typename...> class ContainerType>
void test_map_comparisons() {
    using integral_container = ContainerType<int, int>;
    using two_way_comparable_container = ContainerType<comparisons_testing::TwoWayComparable,
                                                       comparisons_testing::TwoWayComparable>;
    test_comparisons_basic<integral_container>();
    test_comparisons_basic<two_way_comparable_container>();
    test_two_way_comparable_container<two_way_comparable_container>();
}

template <template <typename...> class ContainerType>
void test_set_comparisons() {
    using integral_container = ContainerType<int>;
    using two_way_comparable_container = ContainerType<comparisons_testing::TwoWayComparable>;

    test_comparisons_basic<integral_container>();
    test_comparisons_basic<two_way_comparable_container>();
    test_two_way_comparable_container<two_way_comparable_container>();
}

template <typename Container>
void test_reserve_regression() {
    Container container;

    float lf = container.max_load_factor();
    std::size_t buckets = container.unsafe_bucket_count();
    std::size_t capacity = std::size_t(buckets * lf);

    for (std::size_t elements = 0; elements < capacity; ++elements) {
        container.reserve(elements);
        REQUIRE_MESSAGE(container.unsafe_bucket_count() == buckets,
                        "reserve() should not increase bucket count if the capacity is not reached");
    }

    container.reserve(capacity * 2);
    REQUIRE_MESSAGE(container.unsafe_bucket_count() > buckets, "reserve() should increase bucket count if the capacity is reached");
}

#endif // __TBB_test_common_concurrent_unordered_common
