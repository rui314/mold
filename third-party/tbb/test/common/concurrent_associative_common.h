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

#ifndef __TBB_test_common_concurrent_associative_common_H
#define __TBB_test_common_concurrent_associative_common_H

#include "config.h"

#include "custom_allocators.h"
#include "utils.h"
#include "utils_concurrency_limit.h"
#include "container_move_support.h"
#include "checktype.h"
#include "range_based_for_support.h"
#include "initializer_list_support.h"
#include "node_handling_support.h"
#include "containers_common.h"
#include "test_comparisons.h"
#include "concepts_common.h"
#include <list>
#include <cstring>

#include "oneapi/tbb/parallel_for.h"

// This structure should be specialized for multimap and multiset classes
template <typename T>
struct AllowMultimapping : std::false_type {};

template<typename Table>
inline void CheckAllocator(typename Table::allocator_type& a, size_t expected_allocs, size_t expected_frees,
                           bool exact = true) {
    if(exact) {
        REQUIRE( a.allocations == expected_allocs);
        REQUIRE( a.frees == expected_frees);
    } else {
        REQUIRE( a.allocations >= expected_allocs);
        REQUIRE( a.frees >= expected_frees);
        REQUIRE( a.allocations - a.frees == expected_allocs - expected_frees );
    }
}

// value generator for maps
template <typename Key, typename Value = std::pair<const Key, Key>>
struct ValueFactoryBase {
    using strip_key = typename std::remove_const<Key>::type;
    static Value make( const Key& key ) { return Value(key, key); }
    static Value make( const Key& key, const Key& mapped ) { return Value(key, mapped); }
    static strip_key key( const Value& value ) { return value.first; }
    static strip_key get( const Value& value ) { return strip_key(value.second); }
    template <typename U>
    static U convert( const Value& value ) { return U(value.second); }
};

template <typename T>
struct SpecialTests {
    static void Test() {}
};

// value generator for sets
template <typename Key>
struct ValueFactoryBase<Key, Key> {
    static Key make( const Key& key ) { return key; }
    static Key make( const Key& key, const Key& ) { return key; }
    static Key key( const Key& value ) { return value; }
    static Key get( const Key& value ) { return value; }
    template <typename U>
    static U convert( const Key& value ) { return U(value); }
};

template <typename Container>
struct Value : ValueFactoryBase<typename Container::key_type, typename Container::value_type> {
    template <typename U>
    static bool compare( const typename Container::iterator& it, U val ) {
        return (Value::template convert<U>(*it) == val);
    }
};

template <typename Map>
void SpecialMapTests(){
    Map cont;
    const Map &ccont( cont );

    // mapped_type& operator[](const key_type& k);
    cont[1] = 2;

    // bool empty() const;
    REQUIRE_MESSAGE( !ccont.empty( ), "Concurrent container empty after adding an element" );

    // size_type size() const;
    REQUIRE_MESSAGE( ccont.size( ) == 1, "Concurrent container size incorrect" );
    REQUIRE_MESSAGE( cont[1] == 2, "Concurrent container value incorrect" );

    // mapped_type& at( const key_type& k );
    // const mapped_type& at(const key_type& k) const;
    REQUIRE_MESSAGE( cont.at( 1 ) == 2, "Concurrent container value incorrect" );
    REQUIRE_MESSAGE( ccont.at( 1 ) == 2, "Concurrent container value incorrect" );

    // iterator find(const key_type& k);
    typename Map::iterator it = cont.find( 1 );
    REQUIRE_MESSAGE((it != cont.end( ) && Value<Map>::get( *(it) ) == 2), "Element with key 1 not properly found" );
    cont.unsafe_erase( it );

    it = cont.find( 1 );
    REQUIRE_MESSAGE( it == cont.end( ), "Element with key 1 not properly erased" );
}

template <typename MultiMap>
void CheckMultiMap(MultiMap &m, int *targets, int tcount, int key) {
    std::vector<bool> vfound(tcount,false);
    std::pair<typename MultiMap::iterator, typename MultiMap::iterator> range = m.equal_range( key );
    for(typename MultiMap::iterator it = range.first; it != range.second; ++it) {
        bool found = false;
        for( int i = 0; i < tcount; ++i) {
            if((*it).second == targets[i]) {
                if(!vfound[i])  { // we can insert duplicate values
                    vfound[i] = found = true;
                    break;
                }
            }
        }
        // just in case an extra value in equal_range...
        REQUIRE_MESSAGE(found, "extra value from equal range");
    }
    for(int i = 0; i < tcount; ++i) REQUIRE_MESSAGE(vfound[i], "missing value");
}

template <typename MultiMap>
void MultiMapEraseTests(){
    MultiMap cont1, cont2;

    // Assignment to begin to avoid maybe-uninitialized warning
    typename MultiMap::iterator erased_it = cont1.begin();
    for (int i = 0; i < 10; ++i) {
        if ( i != 1 ) {
            cont1.insert(std::make_pair(1, i));
            cont2.insert(std::make_pair(1, i));
        } else {
            erased_it = cont1.insert(std::make_pair(1, i)).first;
        }
    }

    cont1.unsafe_erase(erased_it);

    REQUIRE_MESSAGE(cont1.size() == cont2.size(), "Incorrect count of elements was erased");
    typename MultiMap::iterator it1 = cont1.begin();
    typename MultiMap::iterator it2 = cont2.begin();

    for (typename MultiMap::size_type i = 0; i < cont2.size(); ++i) {
        REQUIRE_MESSAGE((*(it1++) == *(it2++)), "Multimap repetitive key was not erased properly");
    }
}

template <typename MultiMap>
void SpecialMultiMapTests(){
    int one_values[] = { 7, 2, 13, 23, 13 };
    int zero_values[] = { 4, 9, 13, 29, 42, 111};
    int n_zero_values = sizeof(zero_values) / sizeof(int);
    int n_one_values = sizeof(one_values) / sizeof(int);
    MultiMap cont;
    const MultiMap &ccont( cont );
    // mapped_type& operator[](const key_type& k);
    cont.insert( std::make_pair( 1, one_values[0] ) );

    // bool empty() const;
    REQUIRE_MESSAGE( !ccont.empty( ), "Concurrent container empty after adding an element" );

    // size_type size() const;
    REQUIRE_MESSAGE( ccont.size( ) == 1, "Concurrent container size incorrect" );
    REQUIRE_MESSAGE( (*(cont.begin( ))).second == one_values[0], "Concurrent container value incorrect" );
    REQUIRE_MESSAGE( (*(cont.equal_range( 1 )).first).second == one_values[0], "Improper value from equal_range" );
    REQUIRE_MESSAGE( ((cont.equal_range( 1 )).second == cont.end( )), "Improper iterator from equal_range" );

    cont.insert( std::make_pair( 1, one_values[1] ) );

    // bool empty() const;
    REQUIRE_MESSAGE( !ccont.empty( ), "Concurrent container empty after adding an element" );

    // size_type size() const;
    REQUIRE_MESSAGE( ccont.size( ) == 2, "Concurrent container size incorrect" );
    CheckMultiMap(cont, one_values, 2, 1);

    // insert the other {1,x} values
    for( int i = 2; i < n_one_values; ++i ) {
        cont.insert( std::make_pair( 1, one_values[i] ) );
    }

    CheckMultiMap(cont, one_values, n_one_values, 1);
    REQUIRE_MESSAGE( (cont.equal_range( 1 )).second == cont.end( ), "Improper iterator from equal_range" );

    cont.insert( std::make_pair( 0, zero_values[0] ) );

    // bool empty() const;
    REQUIRE_MESSAGE( !ccont.empty( ), "Concurrent container empty after adding an element" );

    // size_type size() const;
    REQUIRE_MESSAGE( ccont.size( ) == (size_t)(n_one_values+1), "Concurrent container size incorrect" );
    CheckMultiMap(cont, one_values, n_one_values, 1);
    CheckMultiMap(cont, zero_values, 1, 0);
    REQUIRE_MESSAGE( (*cont.find(0)).second == zero_values[0], "Concurrent container value incorrect");
    // insert the rest of the zero values
    for( int i = 1; i < n_zero_values; ++i) {
        cont.insert( std::make_pair( 0, zero_values[i] ) );
    }
    CheckMultiMap(cont, one_values, n_one_values, 1);
    CheckMultiMap(cont, zero_values, n_zero_values, 0);

    // clear, reinsert interleaved
    cont.clear();
    int bigger_num = ( n_one_values > n_zero_values ) ? n_one_values : n_zero_values;
    for( int i = 0; i < bigger_num; ++i ) {
        if(i < n_one_values) cont.insert( std::make_pair( 1, one_values[i] ) );
        if(i < n_zero_values) cont.insert( std::make_pair( 0, zero_values[i] ) );
    }
    CheckMultiMap(cont, one_values, n_one_values, 1);
    CheckMultiMap(cont, zero_values, n_zero_values, 0);

    MultiMapEraseTests<MultiMap>();

}

template <StateTrackableBase::StateValue desired_state, typename T>
void check_value_state( const T& t, std::true_type ) {
    REQUIRE_MESSAGE(is_state_predicate<desired_state>{}(t), "Unexpected value state");
}

template <StateTrackableBase::StateValue desired_state, typename T>
void check_value_state(const T&, std::false_type) {}

template <typename Container, typename CheckElementState, typename Key>
void test_rvalue_insert( Key k1, Key k2 ) {
    Container cont;

    std::pair<typename Container::iterator, bool> ins = cont.insert(Value<Container>::make(k1));
    REQUIRE_MESSAGE(ins.second, "Element 1 has not been inserted");
    REQUIRE_MESSAGE(Value<Container>::get(*ins.first) == k1, "Element 1 has not been inserted");
    check_value_state<StateTrackableBase::MoveInitialized>(*ins.first, CheckElementState{});

    typename Container::iterator it2 = cont.insert(ins.first, Value<Container>::make(k2));
    REQUIRE_MESSAGE(Value<Container>::get(*it2) == k2, "Element 2 has not been inserted");
    check_value_state<StateTrackableBase::MoveInitialized>(*it2, CheckElementState{});

}

namespace emplace_helpers {

template <typename Container, typename Arg, typename Value>
std::pair<typename Container::iterator, bool> call_emplace_impl( Container& c, Arg&& k, Value* ) {
    // Call emplace implementation for sets
    return c.emplace(std::forward<Arg>(k));
}

template <typename Container, typename Arg, typename FirstType, typename SecondType>
std::pair<typename Container::iterator, bool> call_emplace_impl( Container& c, Arg&& k, std::pair<FirstType, SecondType>* ) {
    // Call emplace implementation for maps
    return c.emplace(k, std::forward<Arg>(k));
}

template <typename Container, typename Arg, typename Value>
typename Container::iterator call_emplace_hint_impl( Container& c, typename Container::const_iterator hint,
                                                     Arg&& k, Value* )
{
    // Call emplace_hint implementation for sets
    return c.emplace_hint(hint, std::forward<Arg>(k));
}

template <typename Container, typename Arg, typename FirstType, typename SecondType>
typename Container::iterator call_emplace_hint_impl( Container& c, typename Container::const_iterator hint,
                                                     Arg&& k, std::pair<FirstType, SecondType>* )
{
    // Call emplace_hint implementation for maps
    return c.emplace_hint(hint, k, std::forward<Arg>(k));
}

template <typename Container, typename Arg>
std::pair<typename Container::iterator, bool> call_emplace( Container& c, Arg&& k ) {
    typename Container::value_type* selector = nullptr;
    return call_emplace_impl(c, std::forward<Arg>(k), selector);
}

template <typename Container, typename Arg>
typename Container::iterator call_emplace_hint( Container& c, typename Container::const_iterator hint, Arg&& k ) {
    typename Container::value_type* selector = nullptr;
    return call_emplace_hint_impl(c, hint, std::forward<Arg>(k), selector);
}

} // namespace emplace_helpers

template <typename Container, typename CheckElementState, typename Key>
void test_emplace_insert( Key key1, Key key2 ) {
    Container cont;

    std::pair<typename Container::iterator, bool> ins = emplace_helpers::call_emplace(cont, key1);
    REQUIRE_MESSAGE(ins.second, "Element 1 has not been inserted");
    REQUIRE_MESSAGE(Value<Container>::compare(ins.first, key1), "Element 1 has not been inserted");
    check_value_state<StateTrackableBase::DirectInitialized>(*ins.first, CheckElementState{});

     //if (!AllowMultimapping<Container>::value) {
     //  std::pair<typename Container::iterator, bool> rep_ins = emplace_helpers::call_emplace(cont, key1);
     //  REQUIRE_MESSAGE(!rep_ins.second, "Element 1 has been emplaced twice into the container with unique keys");
     //  REQUIRE(Value<Container>::compare(rep_ins.first, key1));
     //}

    typename Container::iterator it2 = emplace_helpers::call_emplace_hint(cont, ins.first, key2);
    REQUIRE_MESSAGE(Value<Container>::compare(it2, key2), "Element 2 has not been inserted");
    check_value_state<StateTrackableBase::DirectInitialized>(*it2, CheckElementState{});
}

template <typename Container, typename Iterator, typename Range>
std::pair<intptr_t, intptr_t> CheckRecursiveRange( Range range ) {
    std::pair<intptr_t, intptr_t> sum(0, 0); // count, sum

    for (Iterator i = range.begin(); i != range.end(); ++i) {
        ++sum.first;
        sum.second += Value<Container>::get(*i);
    }

    if (range.is_divisible()) {
        Range range2(range, tbb::split{});

        auto sum1 = CheckRecursiveRange<Container, Iterator>(range);
        auto sum2 = CheckRecursiveRange<Container, Iterator>(range2);
        sum1.first += sum2.first; sum1.second += sum2.second;
        REQUIRE_MESSAGE(sum == sum1, "Mismatched ranges afted division");
    }
    return sum;
}

using atomic_byte_type = std::atomic<unsigned char>;

void CheckRange( atomic_byte_type array[], int n, bool allow_multimapping, int odd_count ) {
    if (allow_multimapping) {
        for (int k = 0; k < n; ++k) {
            if (k % 2) {
                REQUIRE(array[k] == odd_count);
            } else {
                REQUIRE(array[k] == 2);
            }
        }
    } else {
        for (int k = 0; k < n; ++k) {
            REQUIRE(array[k] == 1);
        }
    }
}

template <typename T>
void check_equal( const T& cont1, const T& cont2 ) {
    REQUIRE_MESSAGE(cont1 == cont2, "Containers should be equal");
    REQUIRE_MESSAGE(cont2 == cont1, "Containers should be equal");
    REQUIRE_MESSAGE(!(cont1 != cont2), "Containers should not be unequal");
    REQUIRE_MESSAGE(!(cont2 != cont1), "Containers should not be unequal");
}

template <typename T>
void check_unequal( const T& cont1, const T& cont2 ) {
    REQUIRE_MESSAGE(cont1 != cont2, "Containers should be unequal");
    REQUIRE_MESSAGE(cont2 != cont1, "Containers should be unequal");
    REQUIRE_MESSAGE(!(cont1 == cont2), "Containers should not be equal");
    REQUIRE_MESSAGE(!(cont2 == cont1), "Containers should not be equal");
}

// Break value for maps
template <typename First, typename Second>
void break_value( std::pair<First, Second>& value ) {
    ++value.second;
}

template <typename First>
void break_value( std::pair<First, move_support_tests::FooWithAssign>& value ) {
    ++value.second.bar();
}

// Break value for sets
template <typename T>
void break_value( T& value ) {
    ++value;
}

void break_value( move_support_tests::FooWithAssign& value ) {
    ++value.bar();
}

template <typename T>
void test_comparison_operators() {
    T cont;
    check_equal(cont, cont);

    cont.insert(Value<T>::make(1));
    cont.insert(Value<T>::make(2));

    T cont2 = cont;
    check_equal(cont, cont2);

    T cont3;
    check_unequal(cont, cont3);

    T cont4;
    cont4.insert(Value<T>::make(1));
    cont4.insert(Value<T>::make(2));
    check_equal(cont, cont4);

    T cont5;
    cont5.insert(Value<T>::make(1));
    cont5.insert(Value<T>::make(3));
    check_unequal(cont, cont5);

    T cont6;
    cont6.insert(Value<T>::make(1));
    auto value2 = Value<T>::make(2);
    break_value(value2);
    cont6.insert(value2);
    check_unequal(cont, cont6);
}

template <typename Range, typename Container>
void test_empty_container_range(Container&& cont) {
    REQUIRE(cont.empty());
    Range r = cont.range();
    REQUIRE_MESSAGE(r.empty(), "Empty container range should be empty");
    REQUIRE_MESSAGE(!r.is_divisible(), "Empty container range should not be divisible");
    REQUIRE_MESSAGE(r.begin() == r.end(), "Incorrect iterators on empty range");
    REQUIRE_MESSAGE(r.begin() == cont.begin(), "Incorrect iterators on empty range");
}

template<typename T, typename CheckElementState>
void test_basic_common()
{
    T cont;
    const T &ccont(cont);
    CheckNoAllocations(cont);
    // bool empty() const;
    REQUIRE_MESSAGE(ccont.empty(), "Concurrent container is not empty after construction");

    // size_type size() const;
    REQUIRE_MESSAGE(ccont.size() == 0, "Concurrent container is not empty after construction");

    // size_type max_size() const;
    REQUIRE_MESSAGE(ccont.max_size() > 0, "Concurrent container max size is invalid");

    //iterator begin();
    //iterator end();
    REQUIRE_MESSAGE(cont.begin() == cont.end(), "Concurrent container iterators are invalid after construction");
    REQUIRE_MESSAGE(ccont.begin() == ccont.end(), "Concurrent container iterators are invalid after construction");
    REQUIRE_MESSAGE(cont.cbegin() == cont.cend(), "Concurrent container iterators are invalid after construction");

    // Test range for empty container
    using range_type = typename T::range_type;
    using const_range_type = typename T::const_range_type;
    test_empty_container_range<range_type>(cont);
    test_empty_container_range<const_range_type>(cont);
    test_empty_container_range<const_range_type>(ccont);

    T empty_cont;
    const T& empty_ccont = empty_cont;

    for (int i = 0; i < 1000; ++i) {
        empty_cont.insert(Value<T>::make(i));
    }
    empty_cont.clear();

    test_empty_container_range<range_type>(empty_cont);
    test_empty_container_range<const_range_type>(empty_cont);
    test_empty_container_range<const_range_type>(empty_ccont);

    //std::pair<iterator, bool> insert(const value_type& obj);
    std::pair<typename T::iterator, bool> ins = cont.insert(Value<T>::make(1));
    REQUIRE_MESSAGE((ins.second == true && Value<T>::get(*(ins.first)) == 1), "Element 1 has not been inserted properly");

    test_rvalue_insert<T,CheckElementState>(1,2);
    test_emplace_insert<T,CheckElementState>(1,2);

    // bool empty() const;
    REQUIRE_MESSAGE(!ccont.empty(), "Concurrent container is empty after adding an element");

    // size_type size() const;
    REQUIRE_MESSAGE(ccont.size() == 1, "Concurrent container size is incorrect");

    std::pair<typename T::iterator, bool> ins2 = cont.insert(Value<T>::make(1));

    if (AllowMultimapping<T>::value)
    {
        // std::pair<iterator, bool> insert(const value_type& obj);
        REQUIRE_MESSAGE((ins2.second == true && Value<T>::get(*(ins2.first)) == 1), "Element 1 has not been inserted properly");

        // size_type size() const;
        REQUIRE_MESSAGE(ccont.size() == 2, "Concurrent container size is incorrect");

        // size_type count(const key_type& k) const;
        REQUIRE_MESSAGE(ccont.count(1) == 2, "Concurrent container count(1) is incorrect");
        // std::pair<iterator, iterator> equal_range(const key_type& k);
        std::pair<typename T::iterator, typename T::iterator> range = cont.equal_range(1);
        typename T::iterator it;
        it = range.first;
        REQUIRE_MESSAGE((it != cont.end() && Value<T>::get(*it) == 1), "Element 1 has not been found properly");
        unsigned int count = 0;
        for (; it != range.second; it++)
        {
            count++;
            REQUIRE_MESSAGE((Value<T>::get(*it) == 1), "Element 1 has not been found properly");
        }

        REQUIRE_MESSAGE(count == 2, "Range doesn't have the right number of elements");
    }
    else
    {
        // std::pair<iterator, bool> insert(const value_type& obj);
        REQUIRE_MESSAGE((ins2.second == false && ins2.first == ins.first), "Element 1 should not be re-inserted");

        // size_type size() const;
        REQUIRE_MESSAGE(ccont.size() == 1, "Concurrent container size is incorrect");

        // size_type count(const key_type& k) const;
        REQUIRE_MESSAGE(ccont.count(1) == 1, "Concurrent container count(1) is incorrect");

        // std::pair<const_iterator, const_iterator> equal_range(const key_type& k) const;
        // std::pair<iterator, iterator> equal_range(const key_type& k);
        std::pair<typename T::iterator, typename T::iterator> range = cont.equal_range(1);
        typename T::iterator it = range.first;
        REQUIRE_MESSAGE((it != cont.end() && Value<T>::get(*it) == 1), "Element 1 has not been found properly");
        REQUIRE_MESSAGE((++it == range.second), "Range doesn't have the right number of elements");
    }

    // const_iterator find(const key_type& k) const;
    // iterator find(const key_type& k);
    typename T::iterator it = cont.find(1);
    REQUIRE_MESSAGE((it != cont.end() && Value<T>::get(*(it)) == 1), "Element 1 has not been found properly");
    REQUIRE_MESSAGE(ccont.find(1) == it, "Element 1 has not been found properly");

    //bool contains(const key_type&k) const
    REQUIRE_MESSAGE(cont.contains(1), "contains() cannot detect existing element");
    REQUIRE_MESSAGE(!cont.contains(0), "contains() detect not existing element");

    // iterator insert(const_iterator hint, const value_type& obj);
    typename T::iterator it2 = cont.insert(ins.first, Value<T>::make(2));
    REQUIRE_MESSAGE((Value<T>::get(*it2) == 2), "Element 2 has not been inserted properly");

    // T(const T& _Umap)
    T newcont = ccont;

    REQUIRE_MESSAGE((AllowMultimapping<T>{} ? (newcont.size() == 3) : (newcont.size() == 2)), "Copy construction has not copied the elements properly");

    // size_type unsafe_erase(const key_type& k);
    typename T::size_type size;
#if _MSC_VER && __INTEL_COMPILER == 1900
    // The compiler optimizes the next line too aggressively.
#pragma noinline
#endif
    size = cont.unsafe_erase(1);

    REQUIRE_MESSAGE((AllowMultimapping<T>{} ? (size == 2) : (size == 1)), "Erase has not removed the right number of elements");

    // iterator unsafe_erase(iterator position);
    typename T::iterator it4 = cont.unsafe_erase(cont.find(2));

    REQUIRE_MESSAGE((it4 == cont.end() && cont.size() == 0), "Erase has not removed the last element properly");

    // iterator unsafe_erase(const_iterator position);
    cont.insert(Value<T>::make(3));
    typename T::iterator it5 = cont.unsafe_erase(cont.cbegin());
    REQUIRE_MESSAGE((it5 == cont.end() && cont.size() == 0), "Erase has not removed the last element properly");

    // template<class InputIterator> void insert(InputIterator first, InputIterator last);

    cont.insert(newcont.begin(), newcont.end());

    REQUIRE_MESSAGE((AllowMultimapping<T>{} ? (cont.size() == 3) : (cont.size() == 2)), "Range insert has not copied the elements properly");

    // iterator unsafe_erase(const_iterator first, const_iterator last);
    std::pair<typename T::iterator, typename T::iterator> range2 = newcont.equal_range(1);

    newcont.unsafe_erase(range2.first, range2.second);
    REQUIRE_MESSAGE(newcont.size() == 1, "Range erase has not erased the elements properly");

    // void clear();
    newcont.clear();
    REQUIRE_MESSAGE((newcont.begin() == newcont.end() && newcont.size() == 0), "Clear has not cleared the container");

    // void insert(std::initializer_list<value_type> &il);
    newcont.insert( { Value<T>::make( 1 ), Value<T>::make( 2 ), Value<T>::make( 1 ) } );
    if (AllowMultimapping<T>::value) {

        REQUIRE_MESSAGE(newcont.size() == 3, "Concurrent container size is incorrect");
        REQUIRE_MESSAGE(newcont.count(1) == 2, "Concurrent container count(1) is incorrect");
        REQUIRE_MESSAGE(newcont.count(2) == 1, "Concurrent container count(2) is incorrect");
        std::pair<typename T::iterator, typename T::iterator> range = cont.equal_range(1);
        it = range.first;
        // REQUIRE_MESSAGE((it != newcont.end() && Value<T>::get(*it) == 1), "Element 1 has not been found properly");
        REQUIRE_MESSAGE((it != newcont.end()), "iterator" );
        REQUIRE_MESSAGE((Value<T>::get(*it) == 1), "value");
        unsigned int count = 0;
        for (; it != range.second; it++) {
            count++;
            REQUIRE_MESSAGE(Value<T>::get(*it) == 1, "Element 1 has not been found properly");
        }
        REQUIRE_MESSAGE(count == 2, "Range doesn't have the right number of elements");
        range = newcont.equal_range(2); it = range.first;
        REQUIRE_MESSAGE((it != newcont.end() && Value<T>::get(*it) == 2), "Element 2 has not been found properly");
        count = 0;
        for (; it != range.second; it++) {
            count++;
            REQUIRE_MESSAGE(Value<T>::get(*it) == 2, "Element 2 has not been found properly");
        }
        REQUIRE_MESSAGE(count == 1, "Range doesn't have the right number of elements");
    } else {
        REQUIRE_MESSAGE(newcont.size() == 2, "Concurrent container size is incorrect");
        REQUIRE_MESSAGE(newcont.count(1) == 1, "Concurrent container count(1) is incorrect");
        REQUIRE_MESSAGE(newcont.count(2) == 1, "Concurrent container count(2) is incorrect");
        std::pair<typename T::iterator, typename T::iterator> range = newcont.equal_range(1);
        it = range.first;
        REQUIRE_MESSAGE((it != newcont.end() && Value<T>::get(*it) == 1), "Element 1 has not been found properly");
        REQUIRE_MESSAGE((++it == range.second), "Range doesn't have the right number of elements");
        range = newcont.equal_range(2);
        it = range.first;
        REQUIRE_MESSAGE((it != newcont.end() && Value<T>::get(*it) == 2), "Element 2 has not been found properly");
        REQUIRE_MESSAGE((++it == range.second), "Range doesn't have the right number of elements");
    }

    // T& operator=(const T& _Umap)
    newcont = ccont;
    REQUIRE_MESSAGE((AllowMultimapping<T>{} ? (newcont.size() == 3) : (newcont.size() == 2)), "Assignment operator has not copied the elements properly");


    cont.clear();
    CheckNoAllocations(cont);
    for (int i = 0; i < 256; i++)
    {
        std::pair<typename T::iterator, bool> ins3 = cont.insert(Value<T>::make(i));
        REQUIRE_MESSAGE((ins3.second == true && Value<T>::get(*(ins3.first)) == i), "Element 1 has not been inserted properly");
    }
    REQUIRE_MESSAGE(cont.size() == 256, "Wrong number of elements have been inserted");
    REQUIRE(!cont.range().empty());
    REQUIRE(!ccont.range().empty());
    REQUIRE((256 == CheckRecursiveRange<T,typename T::iterator>(cont.range()).first));
    REQUIRE((256 == CheckRecursiveRange<T,typename T::const_iterator>(ccont.range()).first));
    REQUIRE(cont.range().grainsize() > 0);
    REQUIRE(ccont.range().grainsize() > 0);

    // void swap(T&);
    cont.swap(newcont);
    REQUIRE_MESSAGE(newcont.size() == 256, "Wrong number of elements after swap");
    REQUIRE_MESSAGE(newcont.count(200) == 1, "Element with key 200 is not present after swap");
    REQUIRE_MESSAGE(newcont.count(16) == 1, "Element with key 16 is not present after swap");
    REQUIRE_MESSAGE(newcont.count(99) == 1, "Element with key 99 is not present after swap");
    REQUIRE_MESSAGE((AllowMultimapping<T>{} ? (cont.size() == 3) : (cont.size() == 2)), "Assignment operator has not copied the elements properly");

    T newcont_bkp = newcont;
    newcont.swap(newcont);
    REQUIRE_MESSAGE(newcont == newcont_bkp, "Unexpected swap-with-itself behavior");

    test_comparison_operators<T>();

    SpecialTests<T>::Test();
}

template <typename Container>
class FillTable {
    Container& my_table;
    const int my_items;
    bool my_asymptotic;

    using pair_ib = std::pair<typename Container::iterator, bool>;
public:
    FillTable(Container& table, int items, bool asymptotic)
        : my_table(table), my_items(items), my_asymptotic(asymptotic)
    {
        REQUIRE((!(items&1) && items > 100));
    }

    void operator()( int thread_index ) const {
        if (thread_index == 0) { // Fill even keys forward (single thread)
            bool last_inserted = true;

            for (int i = 0; i < my_items; i += 2) {
                int val = my_asymptotic ? 1 : i;
                pair_ib pib = my_table.insert(Value<Container>::make(val));
                REQUIRE_MESSAGE((Value<Container>::get(*(pib.first)) == val),
                                "Element not properly inserted");
                REQUIRE_MESSAGE((last_inserted || !pib.second),
                                "Previous key was not inserted but current key is inserted");
                last_inserted = pib.second;
            }
        } else if (thread_index == 1) { // Fill even keys backward (single thread)
            bool last_inserted = true;

            for (int i = my_items - 2; i >= 0; i -= 2) {
                int val = my_asymptotic ? 1 : i;
                pair_ib pib = my_table.insert(Value<Container>::make(val));
                REQUIRE_MESSAGE((Value<Container>::get(*(pib.first)) == val),
                                "Element not properly inserted");
                REQUIRE_MESSAGE((last_inserted || !pib.second),
                                "Previous key was not inserted but current key is inserted");
                last_inserted = pib.second;
            }
        } else if (!(thread_index & 1)) { // Fill odd keys forward (multiple threads)
            for (int i = 1; i < my_items; i += 2) {
                if (i % 32 == 1 && i + 6 < my_items) {
                    if (my_asymptotic) {
                        auto init = { Value<Container>::make(1), Value<Container>::make(1), Value<Container>::make(1) };
                        my_table.insert(init);
                        REQUIRE_MESSAGE(Value<Container>::get(*my_table.find(1)) == 1, "Element not properly inserted");
                    } else {
                        auto init = { Value<Container>::make(i), Value<Container>::make(i + 2),
                                      Value<Container>::make(i + 4) };
                        my_table.insert(init);
                        REQUIRE_MESSAGE(Value<Container>::get(*my_table.find(i)) == i, "Element i not properly inserted");
                        REQUIRE_MESSAGE(Value<Container>::get(*my_table.find(i + 2)) == i + 2, "Element i + 2 not properly inserted");
                        REQUIRE_MESSAGE(Value<Container>::get(*my_table.find(i + 4)) == i + 4, "Element i + 4 not properly inserted");
                    }
                    i += 4;
                } else {
                    pair_ib pib = my_table.insert(Value<Container>::make(my_asymptotic ? 1 : i));
                    REQUIRE_MESSAGE(Value<Container>::get(*(pib.first)) == (my_asymptotic ? 1 : i), "Element not properly inserted");
                }
            }
        } else { // Check odd keys backward (multiple threads)
            if (!my_asymptotic) {
                bool last_found = false;
                for (int i = my_items - 1; i >= 0; i -= 2) {
                    typename Container::iterator it = my_table.find(i);

                    if (it != my_table.end()) { // found
                        REQUIRE_MESSAGE(Value<Container>::get(*it) == i, "Element not properly inserted");
                        last_found = true;
                    } else {
                        REQUIRE_MESSAGE(!last_found, "Previous key was found, but current was not found");
                    }
                }
            }
        }
    }
}; // class FillTable

template <typename Container, typename Range>
struct ParallelTraverseBody {
    const int n;
    atomic_byte_type* const array;

    ParallelTraverseBody( atomic_byte_type arr[], int num )
        : n(num), array(arr) {}

    void operator()( const Range& range ) const {
        for (auto i = range.begin(); i != range.end(); ++i) {
            int k = static_cast<int>(Value<Container>::key(*i));
            REQUIRE(k == Value<Container>::get(*i));
            REQUIRE(0 <= k);
            REQUIRE(k < n);
            ++array[k];
        }
    }
}; // struct ParallelTraverseBody

template<typename T>
class CheckTable : utils::NoAssign {
    T& table;
public:
    CheckTable(T& t) : NoAssign(), table(t) {}
    void operator()(int i) const {
        int c = (int)table.count(i);
        CHECK_MESSAGE(c, "must exist");
    }
};

template <typename Container>
void test_concurrent_common( bool asymptotic = false ) {
#if __TBB_USE_ASSERT
    int items = 2000;
#else
    int items = 20000;
#endif
    int items_inserted = 0;
    int num_threads = 16;

    Container table;

    if (AllowMultimapping<Container>::value) {
        // TODO: comment
        items = 4 * items / (num_threads + 2);
        items_inserted = items + (num_threads - 2) * items / 4;
    } else {
        items_inserted = items;
    }

    utils::NativeParallelFor(num_threads, FillTable<Container>(table, items, asymptotic));

    REQUIRE(int(table.size()) == items_inserted);

    if (!asymptotic) {
        atomic_byte_type* array = new atomic_byte_type[items];
        std::memset(static_cast<void*>(array), 0, items * sizeof(atomic_byte_type));

        typename Container::range_type r = table.range();

        auto p = CheckRecursiveRange<Container, typename Container::iterator>(r);
        REQUIRE(items_inserted == p.first);

        tbb::parallel_for(r, ParallelTraverseBody<Container, typename Container::range_type>(array, items));
        CheckRange(array, items, AllowMultimapping<Container>::value, (num_threads - 1)/2);

        const Container& const_table = table;
        std::memset(static_cast<void*>(array), 0, items * sizeof(atomic_byte_type));
        typename Container::const_range_type cr = const_table.range();

        p = CheckRecursiveRange<Container, typename Container::const_iterator>(cr);
        REQUIRE(items_inserted == p.first);

        tbb::parallel_for(cr, ParallelTraverseBody<Container, typename Container::const_range_type>(array, items));
        CheckRange(array, items, AllowMultimapping<Container>::value, (num_threads - 1) / 2);
        delete[] array;

        tbb::parallel_for(0, items, CheckTable<Container>(table));
    }

    table.clear();
    // TODO: add check for container allocator counters
}

template <typename ContainerTraits>
void test_rvalue_ref_support() {
    using namespace move_support_tests;
    test_move_constructor<ContainerTraits>();
    test_move_assignment<ContainerTraits>();
#if TBB_USE_EXCEPTIONS
    test_ex_move_constructor<ContainerTraits>();
#endif
}

template <typename Container>
void test_range_based_for_support() {
    using namespace range_based_for_support_tests;

    Container cont;
    const int sequence_length = 100;

    for (int i = 1; i <= sequence_length; ++i) {
        cont.insert(Value<Container>::make(i));
    }

    auto range_based_for_result = range_based_for_accumulate(cont, UnifiedSummer{}, 0);
    auto reference_result = gauss_summ_of_int_sequence(sequence_length);
    REQUIRE_MESSAGE(range_based_for_result == reference_result,
                    "Incorrect accumulated value generated via range based for");
}

template <typename Container>
void test_initializer_list_support( std::initializer_list<typename Container::value_type> init ) {
    using namespace initializer_list_support_tests;

    test_initializer_list_support_without_assign<Container, TestInsertMethod>(init);
    test_initializer_list_support_without_assign<Container, TestInsertMethod>({});
}

template <typename Checker>
void test_set_specific_types() {
    // TODO: add tests for atomics
    Checker check_types;
    const int num = 10;

    // Test int
    std::list<int> arr_int;
    for (int i = 0; i != num; ++i) {
        arr_int.emplace_back(i);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_int);

    // Test reference_wrapper
    std::list<std::reference_wrapper<int>> arr_ref;
    for (auto it = arr_int.begin(); it != arr_int.end(); ++it) {
        arr_ref.emplace_back(*it);
    }
    check_types.template check</*DefCtorPresent = */false>(arr_ref);

    // Test share_ptr
    std::list<std::shared_ptr<int>> arr_shr;
    for (int i = 0; i != num; ++i) {
        arr_shr.emplace_back(std::make_shared<int>(i));
    }
    check_types.template check</*DefCtorPresent = */true>(arr_shr);

    // Test weak_ptr
    std::list<std::weak_ptr<int>> arr_weak;
    std::copy(arr_shr.begin(), arr_shr.end(), std::back_inserter(arr_weak));
    check_types.template check</*DefCtorPresent = */true>(arr_weak);

    // Test std::pair
    std::list<std::pair<int, int>> arr_pairs;
    for (int i = 0; i != num; ++i) {
        arr_pairs.emplace_back(i, i);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_pairs);

    // Test std::basic_string
    std::list<std::basic_string<char, std::char_traits<char>, tbb::tbb_allocator<char>>> arr_strings;
    for (int i = 0; i != num; ++i) {
        arr_strings.emplace_back(i, char(i));
    }
    check_types.template check</*DefCtorPresent = */true>(arr_strings);
}

template <typename Checker>
void test_map_specific_types() {
    // TODO: add tests for int-atomic pairs
    Checker check_types;
    const int num = 10;

    // Test int-int pairs
    std::list<std::pair<const int, int>> arr_int_int_pairs;
    for (int i = 0; i != num; ++i) {
        arr_int_int_pairs.emplace_back(i, num - i);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_int_int_pairs);

    // Test reference_wrapper-int pairs
    std::list<std::pair<const std::reference_wrapper<const int>, int>> arr_ref_int_pairs;
    for (auto& item : arr_int_int_pairs) {
        arr_ref_int_pairs.emplace_back(item.first, item.second);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_ref_int_pairs);

    // Test int-reference_wrapper pairs
    std::list<std::pair<const int, std::reference_wrapper<int>>> arr_int_ref_pairs;
    for (auto& item : arr_int_int_pairs) {
        arr_int_ref_pairs.emplace_back(item.first, item.second);
    }
    check_types.template check</*DefCtorPresent = */false>(arr_int_ref_pairs);

    // Test shared_ptr
    std::list<std::pair<const std::shared_ptr<int>, std::shared_ptr<int>>> arr_shared_pairs;
    for (int i = 0; i != num; ++i) {
        const int num_minus_i = num - i;
        arr_shared_pairs.emplace_back(std::make_shared<int>(i), std::make_shared<int>(num_minus_i));
    }
    check_types.template check</*DefCtorPresent = */true>(arr_shared_pairs);

    // Test weak_ptr
    std::list<std::pair<const std::weak_ptr<int>, std::weak_ptr<int>>> arr_wick_pairs;
    std::copy(arr_shared_pairs.begin(), arr_shared_pairs.end(), std::back_inserter(arr_wick_pairs));
    check_types.template check</*DefCtorPresent = */true>(arr_wick_pairs);

    // Test std::pair
    using pair_key_type = std::pair<int, int>;
    std::list<std::pair<const pair_key_type, int>> arr_pair_int_pairs;
    for (int i = 0; i != num; ++i) {
        arr_pair_int_pairs.emplace_back(pair_key_type{i, i}, i);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_pair_int_pairs);

    // Test std::basic_string
    using tbb_string_key_type = std::basic_string<char, std::char_traits<char>, tbb::tbb_allocator<char>>;
    std::list<std::pair<const tbb_string_key_type, int>> arr_tbb_string_pairs;
    for (int i = 0; i != num; ++i) {
        tbb_string_key_type key(i, char(i));
        arr_tbb_string_pairs.emplace_back(key, i);
    }
    check_types.template check</*DefCtorPresent = */true>(arr_tbb_string_pairs);
}

namespace test {

// For the sake of simplified testing, make std::unique_ptr implicitly convertible to/from the pointer
template <typename T>
class unique_ptr : public std::unique_ptr<T> {
public:
    using pointer = typename std::unique_ptr<T>::pointer;

    unique_ptr( pointer p ) : std::unique_ptr<T>(p) {}
    operator pointer() const { return this->get(); }
}; // class unique_ptr

} // namespace test

namespace std {
template <typename T>
struct hash<test::unique_ptr<T>> {
    std::size_t operator()(const test::unique_ptr<T>& ptr) const {
        return std::hash<std::unique_ptr<T>>{}(ptr);
    }
};
}

template <bool Condition>
struct CallIf {
    template <typename Func>
    void operator()( Func func ) const { func(); }
};

template <>
struct CallIf<false> {
    template <typename Func>
    void operator()( Func ) const {}
};

template <typename Table>
class TestOperatorSquareBrackets {
    using value_type = typename Table::value_type;
    Table& my_c;
    const value_type& my_value;
public:
    TestOperatorSquareBrackets( Table& c, const value_type& value )
        : my_c(c), my_value(value) {}

    void operator()() const {
        utils::IsEqual equal;
        REQUIRE(equal(my_c[my_value.first], my_value.second));
        typename Table::key_type temp_key = my_value.first;
        REQUIRE(equal(my_c[std::move(temp_key)], my_value.second));
    }
}; // class TestOperatorSquareBrackets

template <bool DefCtorPresent, typename Table, typename Value>
void TestSquareBracketsAndAt( Table&, const Value&, /*multimap = */std::true_type ) {
    // operator [] and at are not presented in the multimap
}

template <bool DefCtorPresent, typename Table, typename Value>
void TestSquareBracketsAndAt( Table& c, const Value& value, /*multimap = */std::false_type ) {
    CallIf<DefCtorPresent>()(TestOperatorSquareBrackets<Table>(c, value));
    utils::IsEqual equal;
    REQUIRE(equal(c.at(value.first), value.second));
    // TODO: add test for at exceptions
    const Table& constC = c;
    REQUIRE(equal(constC.at(value.first), value.second));
}

template <bool DefCtorPresent, typename Table, typename Value>
void TestMapSpecificMethods( Table&, const Value& ) {}

template <bool DefCtorPresent, typename Table>
void TestMapSpecificMethods( Table& c, const std::pair<const typename Table::key_type,
                                                       typename Table::mapped_type>& value )
{
    TestSquareBracketsAndAt<DefCtorPresent>(c, value, std::integral_constant<bool, AllowMultimapping<Table>::value>{});
}

template <bool DefCtorPresent, typename Table>
class CheckValue {
    Table& my_c;
public:
    CheckValue( Table& c ) : my_c(c) {}
    void operator()( const typename Table::value_type& value ) {
        using iterator = typename Table::iterator;
        using const_iterator = typename Table::const_iterator;
        const Table& constC = my_c;
        // count
        REQUIRE(my_c.count(Value<Table>::key(value)) == 1);
        // find
        utils::IsEqual equal;
        REQUIRE(equal(*my_c.find(Value<Table>::key(value)), value));
        REQUIRE(equal(*constC.find(Value<Table>::key(value)), value));
        // erase
        REQUIRE(my_c.unsafe_erase(Value<Table>::key(value)) != 0);
        REQUIRE(my_c.unsafe_erase(Value<Table>::key(value)) == 0);
        // insert
        std::pair<iterator, bool> res = my_c.insert(value);
        REQUIRE(equal(*res.first, value));
        REQUIRE(res.second);
        // erase
        iterator it = res.first;
        ++it;
        REQUIRE(my_c.unsafe_erase(res.first) == it);
        // insert
        REQUIRE(equal(*my_c.insert(my_c.begin(), value), value));
        // equal_range
        std::pair<iterator, iterator> r1 = my_c.equal_range(Value<Table>::key(value));
        REQUIRE((equal(*r1.first, value) && ++r1.first == r1.second));
        std::pair<const_iterator, const_iterator> r2 = constC.equal_range(Value<Table>::key(value));
        REQUIRE((equal(*r2.first, value) && ++r2.first == r2.second));

        TestMapSpecificMethods<DefCtorPresent>(my_c, value);
    }
}; // class CheckValue

namespace detail {

#if (__INTEL_COMPILER || __clang__ ) && __TBB_GLIBCXX_VERSION && __TBB_GLIBCXX_VERSION < 40900
template <typename T>
struct assignable_atomic : std::atomic<T> {
    using std::atomic<T>::operator=;
    assignable_atomic& operator=(const assignable_atomic& a) {
        store(a.load(std::memory_order_relaxed), std::memory_order_relaxed);
    }
};

template <typename T>
using atomic_type = assignable_atomic<T>;
#else
template <typename T>
using atomic_type = std::atomic<T>;
#endif
}

template <typename Value>
class TestRange {
    const std::list<Value>& my_lst;
    std::vector<detail::atomic_type<bool>>& my_marks;
public:
    TestRange( const std::list<Value>& lst, std::vector<detail::atomic_type<bool>>& marks )
        : my_lst(lst), my_marks(marks)
    {
        std::fill(my_marks.begin(), my_marks.end(), false);
    }

    template <typename Range>
    void operator()( const Range& r ) const {
        do_test_range(r.begin(), r.end());
    }

    template <typename Iterator>
    void do_test_range( Iterator i, Iterator j ) const {
        for (Iterator it = i; it != j;) {
            Iterator prev_it = it++;
            auto it2 = std::search(my_lst.begin(), my_lst.end(), prev_it, it, utils::IsEqual());
            REQUIRE(it2 != my_lst.end());
            auto dist = std::distance(my_lst.begin(), it2);
            REQUIRE(!my_marks[dist]);
            my_marks[dist] = true;
        }
    }
}; // class TestRange

template <bool DefCtorPresent, typename Table>
void CommonExamine( Table c, const std::list<typename Table::value_type>& lst ) {
    using value_type = typename Table::value_type;

    if (!(!c.empty() && c.size() == lst.size() && c.max_size() >= c.size())) {
        std::cout << std::boolalpha;
        std::cout << "Empty? " << c.empty() << std::endl;
        std::cout << "sizes equal? " << (c.size() == lst.size()) << std::endl;
        std::cout << "\t" << c.size() << std::endl;
        std::cout << "\t" << lst.size() << std::endl;
        std::cout << "Max size greater? " << (c.max_size() >= c.size()) << std::endl;
    }
    REQUIRE((!c.empty() && c.size() == lst.size() && c.max_size() >= c.size()));

    std::for_each(lst.begin(), lst.end(), CheckValue<DefCtorPresent, Table>(c));

    std::vector<detail::atomic_type<bool>> marks(lst.size());

    TestRange<value_type>(lst, marks).do_test_range(c.begin(), c.end());
    REQUIRE(std::find(marks.begin(), marks.end(), false) == marks.end());

    const Table constC = c;
    REQUIRE(c.size() == constC.size());

    TestRange<value_type>(lst, marks).do_test_range(c.begin(), c.end());
    REQUIRE(std::find(marks.begin(), marks.end(), false) == marks.end());

    tbb::parallel_for(c.range(), TestRange<value_type>(lst, marks));
    REQUIRE(std::find(marks.begin(), marks.end(), false) == marks.end());

    tbb::parallel_for(constC.range(), TestRange<value_type>(lst, marks));
    REQUIRE(std::find(marks.begin(), marks.end(), false) == marks.end());

    Table c2;
    auto begin5 = lst.begin();
    std::advance(begin5, 5);
    c2.insert(lst.begin(), begin5);
    std::for_each(lst.begin(), begin5, CheckValue<DefCtorPresent, Table>(c2));

    c2.swap(c);
    REQUIRE(c2.size() == lst.size());
    REQUIRE(c.size() == 5);

    std::for_each(lst.begin(), lst.end(), CheckValue<DefCtorPresent, Table>(c2));

    c2.clear();
    REQUIRE(c2.size() == 0);

    auto alloc = c.get_allocator();
    value_type* ptr = alloc.allocate(1);
    REQUIRE(ptr != nullptr);
    alloc.deallocate(ptr, 1);
}

template <typename ContainerTraits>
void test_scoped_allocator() {
    using allocator_data_type = AllocatorAwareData<std::scoped_allocator_adaptor<std::allocator<int>>>;
    using basic_allocator_type = std::scoped_allocator_adaptor<std::allocator<allocator_data_type>>;
    using container_value_type = typename ContainerTraits::template container_value_type<allocator_data_type>;
    using allocator_type = typename std::allocator_traits<basic_allocator_type>::template rebind_alloc<container_value_type>;
    using container_type = typename ContainerTraits::template container_type<allocator_data_type, allocator_type>;

    allocator_type allocator;
    allocator_data_type key1(1, allocator);
    allocator_data_type key2(2, allocator);

    container_value_type value1 = Value<container_type>::make(key1);
    container_value_type value2 = Value<container_type>::make(key2);

    auto init_list = { value1, value2 };

    container_type c1(allocator);
    container_type c2(allocator);

    allocator_data_type::activate();

    emplace_helpers::call_emplace(c1, key1);
    emplace_helpers::call_emplace(c2, std::move(key2));

    c1.clear();
    c2.clear();

    c1.insert(value1);
    c2.insert(std::move(value2));

    c1.clear();
    c2.clear();

    c1.insert(init_list);
    c2.insert(value1);

    c1 = c2;
    c2 = std::move(c1);

    allocator_data_type::deactivate();
}


struct int_key {
    int my_item;
    int_key(int i) : my_item(i) {}
};

bool operator==(const int_key& ik, int i) { return ik.my_item == i; }
bool operator==(int i, const int_key& ik) { return i == ik.my_item; }
bool operator==(const int_key& ik1, const int_key& ik2) { return ik1.my_item == ik2.my_item; }

bool operator<( const int_key& ik, int i ) { return ik.my_item < i; }
bool operator<( int i, const int_key& ik ) { return i < ik.my_item; }
bool operator<( const int_key& ik1, const int_key& ik2 ) { return ik1.my_item < ik2.my_item; }

struct char_key {
    const char* my_item;
    char_key(const char* c) : my_item(c) {}

    const char& operator[] (std::size_t pos) const {
        return my_item[pos];
    }

    std::size_t size() const {
        return std::strlen(my_item);
    }
};

bool operator==(const char_key& ck, std::string c) {
    std::size_t i = 0;
    while (ck[i] != '\0' && i < c.size() && ck[i] == c[i]) { ++i;}
    return c.size() == i && ck[i] == '\0';
}
bool operator==(std::string c, const char_key& ck) {
    return ck == c;
}
bool operator==(const char_key& ck1, const char_key& ck2) {
    std::size_t i = 0;
    while (ck1[i] != '\0' && ck2[i] != '\0' && ck1[i] == ck2[i]) { ++i;}
    return ck1[i] == ck2[i];
}

bool operator<( const char_key& ck, std::string c ) {
    return std::lexicographical_compare(ck.my_item, ck.my_item + ck.size(), c.begin(), c.end());
}

bool operator<(std::string c, const char_key& ck) {
    return std::lexicographical_compare(c.begin(), c.end(), ck.my_item, ck.my_item + ck.size());
}

bool operator<( const char_key& ck1, const char_key& ck2 ) {
    return std::lexicographical_compare(ck1.my_item, ck1.my_item + ck1.size(), ck2.my_item, ck2.my_item + ck2.size());
}

struct equal_to {
    using is_transparent = int;
    template <typename T, typename W>
    bool operator()(const T &lhs, const W &rhs) const {
        return lhs == rhs;
    }
};

struct hash_with_transparent_key_equal {
    template <typename T>
    size_t operator()(const T& key) const { return hash(key); }
    using transparent_key_equal = equal_to;
    int prime = 433494437;
    int first_factor = 41241245;
    int second_factor = 2523422;

    size_t hash(const int& key) const { return (first_factor * key + second_factor) % prime; }

    size_t hash(const int_key& key) const { return (first_factor * key.my_item + second_factor) % prime; }

    size_t hash(const std::string& key) const {
        int sum = 0;
        for (auto it = key.begin(); it != key.end(); ++it) {
            sum += first_factor * (*it) + second_factor;
        }
        return sum % prime;
    }

    size_t hash(const char_key& key) const {
        int sum = 0;
        int i = 0;
        while (key[i] != '\0') {
            sum += first_factor * key[i++] + second_factor;
        }
        return sum % prime;
    }

};

template <typename Container>
void check_heterogeneous_functions_key_int_impl() {
    static_assert(std::is_same<typename Container::key_type, int>::value,
                  "incorrect key_type for heterogeneous lookup test");
    // Initialization
    Container c;
    int size = 10;
    for (int i = 0; i < size; i++) {
        c.insert(Value<Container>::make(i));
    }
    // Insert first duplicated element for multicontainers
    if (AllowMultimapping<Container>::value) {
        c.insert(Value<Container>::make(0));
    }

    // Look up testing
    for (int i = 0; i < size; i++) {
        int_key k(i);
        int key = i;
        REQUIRE_MESSAGE(c.find(k) == c.find(key), "Incorrect heterogeneous find return value");
        REQUIRE_MESSAGE(c.count(k) == c.count(key), "Incorrect heterogeneous count return value");
    }

    // unsafe_extract testing
    for (int i = 0; i < size; i++) {
        Container extract_c = c;
        int_key int_k(i);
        auto int_key_extract = extract_c.unsafe_extract(int_k);
        if (!AllowMultimapping<Container>::value) {
            REQUIRE_MESSAGE(extract_c.find(int_k) == extract_c.end(), "Key exists after extract");
        }
        REQUIRE_MESSAGE(!int_key_extract.empty(), "Empty node with exists key");
        REQUIRE_MESSAGE(node_handling_tests::compare_handle_getters(int_key_extract, Value<Container>::make(i)), "Incorrect node");
    }

    // unsafe_extract not exists key
    auto not_exists = c.unsafe_extract(int_key(100));
    REQUIRE_MESSAGE(not_exists.empty(), "Not empty node with not exists key");

    // multimap unsafe_extract testing
    if (AllowMultimapping<Container>::value) {
        Container extract_m;
        for (int i = 0; i < size; i++) {
            extract_m.insert(Value<Container>::make(i));
            extract_m.insert(Value<Container>::make(i, i + 1));
        }
        for (int i = 0; i < size; i++) {
            int_key int_k(i);
            auto int_key_extract = extract_m.unsafe_extract(int_k);
            REQUIRE_MESSAGE(!int_key_extract.empty(), "Empty node with exists key");
            REQUIRE_MESSAGE((node_handling_tests::compare_handle_getters(int_key_extract, Value<Container>::make(i, i)) ||
                    node_handling_tests::compare_handle_getters(int_key_extract, Value<Container>::make(i, i + 1))), "Incorrect node");
            REQUIRE_MESSAGE(extract_m.find(int_k) != extract_m.end(), "All nodes for key deleted");
        }
    }

    // Erase testing
    for (int i = 0; i < size; i++) {
        auto count_before_erase = c.count(i);
        auto result = c.unsafe_erase(int_key(i));
        REQUIRE_MESSAGE(count_before_erase == result,"Incorrect erased elements count");
        REQUIRE_MESSAGE(c.count(i) == 0, "Some elements was not erased");
    }

}

template <typename Container>
void check_heterogeneous_functions_key_string_impl() {
    static_assert(std::is_same<typename Container::key_type, std::string>::value,
                  "incorrect key_type for heterogeneous lookup test");
    // Initialization
    std::vector<const char*> keys{"key1", "key2", "key3", "key4",
        "key5", "key6", "key7", "key8", "key9", "key10"};
    std::vector<const char*> values{"value1", "value2", "value3", "value4",
        "value5", "value6", "value7", "value8", "value9", "value10", "value11"};
    Container c;
    for (auto it = keys.begin(); it != keys.end(); ++it) {
        c.insert(Value<Container>::make(*it));
    }
    // Insert first duplicated element for multicontainers
    if (AllowMultimapping<Container>::value) {
        c.insert(Value<Container>::make(*keys.begin()));
    }

    // Look up testing
    for (auto it = keys.begin(); it != keys.end(); ++it) {
        std::string key = *it;
        char_key k{*it};
        REQUIRE_MESSAGE(c.find(k) == c.find(key), "Incorrect heterogeneous find return value");
        REQUIRE_MESSAGE(c.count(k) == c.count(key), "Incorrect heterogeneous count return value");
    }

    // unsafe_extract testing
    for (auto it = keys.begin(); it != keys.end(); ++it) {
        Container extract_c = c;
        char_key k{*it};
        auto char_key_extract = extract_c.unsafe_extract(k);
        REQUIRE_MESSAGE(!char_key_extract.empty(), "Empty node with exists key");
        REQUIRE_MESSAGE(node_handling_tests::compare_handle_getters(char_key_extract, Value<Container>::make(*it)), "Incorrect node");
    }

    // unsafe_extract not exists key
    auto not_exists = c.unsafe_extract(char_key("not exists"));
    REQUIRE_MESSAGE(not_exists.empty(), "Not empty node with not exists key");

    // multimap unsafe_extract testing
    if (AllowMultimapping<Container>::value){
        Container extract_m;
        for (std::size_t i = 0; i < keys.size(); i++) {
            extract_m.insert(Value<Container>::make(keys[i], values[i]));
            extract_m.insert(Value<Container>::make(keys[i], values[i + 1]));
        }
        for (std::size_t i = 0; i < keys.size(); i++) {
            char_key char_k(keys[i]);
            auto char_key_extract = extract_m.unsafe_extract(char_k);
            REQUIRE_MESSAGE(!char_key_extract.empty(), "Empty node with exists key");
            REQUIRE_MESSAGE((node_handling_tests::compare_handle_getters(char_key_extract, Value<Container>::make(keys[i], values[i])) ||
                    node_handling_tests::compare_handle_getters(char_key_extract, Value<Container>::make(keys[i], values[i + 1]))), "Incorrect node");
            REQUIRE_MESSAGE(extract_m.find(char_k) != extract_m.end(), "All nodes for key deleted");
        }
    }

    // Erase testing
    for (auto it = keys.begin(); it != keys.end(); ++it) {
        std::string key = *it;
        char_key k{*it};
        auto count_before_erase = c.count(key);
        auto result = c.unsafe_erase(k);
        REQUIRE_MESSAGE(count_before_erase==result,"Incorrect erased elements count");
        REQUIRE_MESSAGE(c.count(key)==0, "Some elements was not erased");
    }
}

struct CountingKey {
    static std::size_t counter;

    CountingKey() { ++counter; }
    CountingKey( const CountingKey& ) { ++counter; }
    ~CountingKey() {}
    CountingKey& operator=( const CountingKey& ) { return *this; }

    static void reset() { counter = 0; }
};

std::size_t CountingKey::counter;

namespace std {
template <>
struct hash<CountingKey> {
    std::size_t operator()( const CountingKey& ) const { return 0; }
};
}

bool operator==( const CountingKey&, const CountingKey& ) { return true; }

bool operator<( const CountingKey&, const CountingKey& ) { return true; }

struct int_constructible_object {
    int_constructible_object(int k) : key(k) {}
    int key;
}; // struct int_constructible_object

bool operator==( const int_constructible_object& lhs, const int_constructible_object rhs ) {
    return lhs.key == rhs.key;
}

// A test for
// template <typename P> insert(P&&) in maps
template <template <typename...> class Container>
void test_insert_by_generic_pair() {
    using value_type = std::pair<const int, int_constructible_object>;
    using generic_pair_type = std::pair<int, int>;

    static_assert(std::is_constructible<value_type, generic_pair_type>::value,
                  "Incorrect test setup");

    Container<int, int_constructible_object> cont1, cont2;
    using iterator = typename Container<int, int_constructible_object>::iterator;

    for (int i = 0; i != 10; ++i) {
        std::pair<iterator, bool> res_generic_insert = cont1.insert(generic_pair_type(1, i));
        std::pair<iterator, bool> res_value_insert = cont2.insert(value_type(1, int_constructible_object(i)));

        REQUIRE_MESSAGE(*res_generic_insert.first == *res_value_insert.first, "Insert by generic pair returned wrong iterator");
        REQUIRE_MESSAGE(res_generic_insert.second == res_value_insert.second, "Insert by generic pair returned wrong insertion value");
    }

    for (int i = 0; i != 10; ++i) {
        iterator res_generic_insert_hint = cont1.insert(cont1.cbegin(), generic_pair_type(2, i));
        iterator res_value_insert_hint = cont2.insert(cont2.cbegin(), value_type(2, int_constructible_object(i)));

        REQUIRE_MESSAGE(*res_generic_insert_hint == *res_value_insert_hint, "Hinted insert by generic pair returned wrong iterator");
    }

    Container<CountingKey, int_constructible_object> counting_cont;
    using counting_generic_pair = std::pair<CountingKey, int>;

    static_assert(std::is_constructible<typename decltype(counting_cont)::value_type, counting_generic_pair>::value,
                  "Incorrect test setup");

    counting_generic_pair counting_pair(CountingKey{}, 1);
    CountingKey::reset();

    counting_cont.insert(counting_pair);
    REQUIRE_MESSAGE(CountingKey::counter == 1, "Only one element should be constructed in-place during the generic pair insertion");

    CountingKey::reset();
}

template <typename Container>
void test_swap_not_always_equal_allocator() {
    static_assert(std::is_same<typename Container::allocator_type, NotAlwaysEqualAllocator<typename Container::value_type>>::value,
                  "Incorrect allocator in not always equal test");
    Container c1{};
    Container c2{Value<Container>::make(1), Value<Container>::make(2)};

    Container c1_copy = c1;
    Container c2_copy = c2;

    c1.swap(c2);

    REQUIRE_MESSAGE(c1 == c2_copy, "Incorrect swap with not always equal allocator");
    REQUIRE_MESSAGE(c2 == c1_copy, "Incorrect swap with not always equal allocator");
}

#if TBB_USE_EXCEPTIONS
template <typename Container>
void test_exception_on_copy_ctor() {
    Container c1;
    c1.emplace(Value<Container>::make(ThrowOnCopy{}));

    using container_allocator_type = std::allocator<Container>;
    using alloc_traits = std::allocator_traits<container_allocator_type>;
    container_allocator_type container_allocator;
    Container* c2_ptr = alloc_traits::allocate(container_allocator, 1);

    ThrowOnCopy::activate();
    // Test copy ctor
    try {
        alloc_traits::construct(container_allocator, c2_ptr, c1);
    } catch ( int error_code ) {
        REQUIRE_MESSAGE(error_code == ThrowOnCopy::error_code(), "Incorrect code was thrown");
    }

    REQUIRE_MESSAGE(c2_ptr->empty(), "Incorrect container state after throwing copy constructor");

    alloc_traits::deallocate(container_allocator, c2_ptr, 1);
    c2_ptr = alloc_traits::allocate(container_allocator, 1);

    // Test copy ctor with allocator
    try {
        auto value_allocator = c1.get_allocator();
        alloc_traits::construct(container_allocator, c2_ptr, c1, value_allocator);
    } catch( int error_code ) {
        REQUIRE_MESSAGE(error_code == ThrowOnCopy::error_code(), "Incorrect code was thrown");
    }

    REQUIRE_MESSAGE(c2_ptr->empty(), "Incorrect container state after throwing copy ctor with allocator");
    alloc_traits::deallocate(container_allocator, c2_ptr, 1);
    ThrowOnCopy::deactivate();
}
#endif // TBB_USE_EXCEPTIONS

#endif // __TBB_test_common_concurrent_associative_common_H
