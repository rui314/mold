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

// Tests for compatibility with the host's STL.

#ifndef __TBB_test_common_allocator_stl_test_H_
#define __TBB_test_common_allocator_stl_test_H_

#include "common/test.h"

template<typename Container>
void TestSequence(const typename Container::allocator_type &a) {
    constexpr auto iter_count = 1000;
    Container c(a);
    for(int i = 0; i < iter_count; ++i){
        c.push_back(i * i);
    }
    typename Container::const_iterator p = c.begin();
    for(int i = 0; i < iter_count; ++i) {
        REQUIRE(*p == i*i);
        ++p;
    }
    // regression test against compilation error for GCC 4.6.2
    c.resize(1000);
}

template<typename Set>
void TestSet(const typename Set::allocator_type &a) {
    Set s(typename Set::key_compare(), a);
    using value_type = typename Set::value_type;
    for(int i = 0; i < 100; ++i)
        s.insert(value_type(3 * i));
    for( int i = 0; i < 300; ++i ) {
        REQUIRE(s.erase(i) == size_t(i % 3 == 0));
    }
}

template<typename Map>
void TestMap(const typename Map::allocator_type &a) {
    Map m(typename Map::key_compare(), a);
    using value_type = typename Map::value_type;
    for(int i = 0; i < 100; ++i)
        m.insert(value_type(i,i*i));
    for(int i=0; i < 100; ++i)
        REQUIRE(m.find(i)->second == i * i);
}

#include <deque>
#include <list>
#include <map>
#include <set>
#include <vector>

struct MoveOperationTracker {
    int my_value;

    MoveOperationTracker(int value = 0) : my_value(value) {}
    MoveOperationTracker(const MoveOperationTracker&) {
        REQUIRE_MESSAGE(false, "Copy constructor is called");
    }
    MoveOperationTracker(MoveOperationTracker&& m) noexcept : my_value( m.my_value ) {
    }
    MoveOperationTracker& operator=(MoveOperationTracker const&) {
        REQUIRE_MESSAGE(false, "Copy assignment operator is called");
        return *this;
    }
    MoveOperationTracker& operator=(MoveOperationTracker&& m) noexcept {
        my_value = m.my_value;
        return *this;
    }

    bool operator==(int value) const {
        return my_value == value;
    }

    bool operator==(const MoveOperationTracker& m) const {
        return my_value == m.my_value;
    }
};

template<typename Allocator>
void TestAllocatorWithSTL(const Allocator &a = Allocator()) {

// Allocator type conversion section
    using Ai = typename std::allocator_traits<Allocator>::template rebind_alloc<int>;
    using Acii = typename std::allocator_traits<Allocator>::template rebind_alloc<std::pair<const int, int> >;
#if _MSC_VER && _CPPLIB_VER < 650
    using Aci = typename std::allocator_traits<Allocator>::template rebind_alloc<const int>;
    using Aii = typename std::allocator_traits<Allocator>::template rebind_alloc<std::pair<int, int> >;
#endif // _MSC_VER

    // Sequenced containers
    TestSequence<std::deque <int,Ai> >(a);
    TestSequence<std::list  <int,Ai> >(a);
    TestSequence<std::vector<int,Ai> >(a);

    using Amot = typename std::allocator_traits<Allocator>::template rebind_alloc<MoveOperationTracker>;
    TestSequence<std::deque <MoveOperationTracker, Amot> >(a);
    TestSequence<std::list  <MoveOperationTracker, Amot> >(a);
    TestSequence<std::vector<MoveOperationTracker, Amot> >(a);

    // Associative containers
    TestSet<std::set     <int, std::less<int>, Ai> >(a);
    TestSet<std::multiset<int, std::less<int>, Ai> >(a);
    TestMap<std::map     <int, int, std::less<int>, Acii> >(a);
    TestMap<std::multimap<int, int, std::less<int>, Acii> >(a);

#if _MSC_VER && _CPPLIB_VER < 650
    // Test compatibility with Microsoft's implementation of std::allocator for some cases that
    // are undefined according to the ISO standard but permitted by Microsoft.
    TestSequence<std::deque <const int,Aci> >(a);
#if _CPPLIB_VER>=500
    TestSequence<std::list  <const int,Aci> >(a);
#endif
    TestSequence<std::vector<const int,Aci> >(a);
    TestSet<std::set<const int, std::less<int>, Aci> >(a);
    TestMap<std::map<int, int, std::less<int>, Aii> >(a);
    TestMap<std::map<const int, int, std::less<int>, Acii> >(a);
    TestMap<std::multimap<int, int, std::less<int>, Aii> >(a);
    TestMap<std::multimap<const int, int, std::less<int>, Acii> >(a);
#endif /* _MSC_VER */
}
#endif // __TBB_test_common_allocator_stl_test_H_
