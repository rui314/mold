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

//! \file test_intrusive_list.cpp
//! \brief Test for [internal] functionality

#include "common/test.h"
#include "common/utils.h"
#include "../../src/tbb/intrusive_list.h"

using tbb::detail::r1::intrusive_list_node;

// Machine word filled with repeated pattern of FC bits
const uintptr_t NoliMeTangere = ~uintptr_t(0)/0xFF*0xFC;

struct VerificationBase : utils::NoAfterlife {
    uintptr_t m_Canary;
    VerificationBase() : m_Canary(NoliMeTangere) {}
}; // struct VerificationBase

struct DataItemWithInheritedNodeBase : intrusive_list_node {
    int m_Data;

    DataItemWithInheritedNodeBase( int value ) : m_Data(value) {}

    int Data() const { return m_Data; }
}; // struct DataItemWithInheritedNodeBase

struct DataItemWithInheritedNode : VerificationBase, DataItemWithInheritedNodeBase {
    friend class tbb::detail::r1::intrusive_list<DataItemWithInheritedNode>;

    DataItemWithInheritedNode( int value ) : DataItemWithInheritedNodeBase(value) {}
}; // struct DataItemWithInheritedNode

struct DataItemWithMemberNodeBase {
    int m_Data;

    // Cannot be used be member_intrusive_list to form lists of objects derived from DataItemBase
    intrusive_list_node m_BaseNode;

    DataItemWithMemberNodeBase( int value ) : m_Data(value) {}

    int Data() const { return m_Data; }
}; // struct DataItemWithMemberNodeBase

struct DataItemWithMemberNodes : VerificationBase, DataItemWithMemberNodeBase {
    intrusive_list_node m_Node;

    DataItemWithMemberNodes( int value ) : DataItemWithMemberNodeBase(value) {}
}; // struct DataItemWithMemberNodes

using intrusive_list1 = tbb::detail::r1::intrusive_list<DataItemWithInheritedNode>;
using intrusive_list2 = tbb::detail::r1::memptr_intrusive_list<DataItemWithMemberNodes,
                                                               DataItemWithMemberNodeBase,
                                                               &DataItemWithMemberNodeBase::m_BaseNode>;

using intrusive_list3 = tbb::detail::r1::memptr_intrusive_list<DataItemWithMemberNodes,
                                                               DataItemWithMemberNodes,
                                                               &DataItemWithMemberNodes::m_Node>;

const int NumElements = 256 * 1024;

// Iterates through the list forward and backward checking the validity of values stored by the list nodes
template <typename List, typename Iterator>
void check_list_nodes( List& il, int value_step ) {
    REQUIRE_MESSAGE(il.size() == unsigned(NumElements / value_step), "Wrong size of the list");
    REQUIRE_MESSAGE(!il.empty(), "Incorrect result of empty() or the list is corrupted");

    int i;
    Iterator it = il.begin();

    Iterator it_default;
    REQUIRE_MESSAGE(it_default != it, "Incorrect default constructed intrusive_list::iterator");

    for ( i = value_step - 1; it != il.end(); ++it, i += value_step ) {
        REQUIRE_MESSAGE(it->Data() == i, "Unexpected node value while iterating forward");
        REQUIRE_MESSAGE(it->m_Canary == NoliMeTangere, "Memory corruption");
    }

    REQUIRE_MESSAGE(i == NumElements + value_step - 1, "Wrong number of list elements while iterating forward");
    it = il.end();

    for ( i = NumElements - 1, it--; it != il.end(); --it, i -= value_step ) {
        REQUIRE_MESSAGE(it->Data() == i, "Unexpected node value while iterating backward");
        REQUIRE_MESSAGE(it->m_Canary == NoliMeTangere, "Memory corruption");
    }
    REQUIRE_MESSAGE(i == -1, "Wrong number of list elements while iterating backward");
}

template <typename List, typename Item>
void test_list_operations() {
    using iterator = typename List::iterator;

    List il;

    for (int i = NumElements - 1; i >= 0; --i) {
        il.push_front(*new Item(i));
    }
    check_list_nodes<const List, typename List::const_iterator>(il, 1);
    iterator it = il.begin();

    for (;it != il.end(); ++it) {
        Item& item = *it;
        it = il.erase(it); // also advances the iterator
        delete &item;
    }

    check_list_nodes<List, iterator>(il, 2);
    for (it = il.begin(); it != il.end(); ++it) {
        Item& item = *it;
        il.remove(*it++); // extra advance here as well
        delete &item;
    }

    check_list_nodes<List, iterator>(il, 4);
    for (it = il.begin(); it != il.end();) {
        Item& item = *it++; // the iterator advances only here
        il.remove(item);
        delete &item;
    }
    REQUIRE_MESSAGE(il.size() == 0, "The list has wrong size or not all items were removed");
    REQUIRE_MESSAGE(il.empty(), "Incorrect result of empty() or not all items were removed");
}

// TODO: tests for intrusive_list assertions were not ported

//! \brief \ref error_guessing
TEST_CASE("Test tbb::detail::r1::intrusive_list operations") {
    test_list_operations<intrusive_list1, DataItemWithInheritedNode>();
}

//! \brief \ref error_guessing
TEST_CASE("Test tbb::detail::r1::memptr_intrusive_list operations") {
    test_list_operations<intrusive_list2, DataItemWithMemberNodes>();
    test_list_operations<intrusive_list3, DataItemWithMemberNodes>();
}
