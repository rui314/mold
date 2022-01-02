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

#ifndef __TBB_test_common_node_handling_support_H
#define __TBB_test_common_node_handling_support_H

#include "common/test.h"
#include <utility>

template <typename T>
struct AllowMultimapping;

template <typename Container>
struct Value;

namespace node_handling_tests {

template <typename Handle>
bool compare_handle_getters( const Handle& node, const std::pair<typename Handle::key_type, typename Handle::mapped_type>& value  ) {
    return node.key() == value.first && node.mapped() == value.second;
}

template <typename Handle>
bool compare_handle_getters( const Handle& node, const typename Handle::value_type& value ) {
    return node.value() == value;
}

template <typename Handle>
void set_node_handle_value( Handle& node, const std::pair<typename Handle::key_type, typename Handle::mapped_type>& value ) {
    node.key() = value.first;
    node.mapped() = value.second;
}

template <typename Handle>
void set_node_handle_value( Handle& node, const typename Handle::value_type& value ) {
    node.value() = value;
}

template <typename NodeType>
void test_node_handle_traits() {
    REQUIRE_MESSAGE(!std::is_copy_constructible<NodeType>::value,
                    "Node handle: handle should not be copy constructible");
    REQUIRE_MESSAGE(!std::is_copy_assignable<NodeType>::value,
                    "Node handle: handle should not be copy assignable");
    REQUIRE_MESSAGE(std::is_move_constructible<NodeType>::value,
                    "Node handle: handle should be move constructible");
    REQUIRE_MESSAGE(std::is_move_assignable<NodeType>::value,
                    "Node handle: handle should be move assignable");
    REQUIRE_MESSAGE(std::is_default_constructible<NodeType>::value,
                    "Node handle: handle should be default constructible");
    REQUIRE_MESSAGE(std::is_destructible<NodeType>::value,
                    "Node handle: handle should be destructible");
}

template <typename Container>
void test_node_handle( Container test_table ) {
    REQUIRE_MESSAGE(test_table.size() > 1, "Node handle: Container must contain 2 or more elements");
    // Initialization
    using node_type = typename Container::node_type;

    test_node_handle_traits<node_type>();

    // Default ctor and empty initialization
    node_type nh;
    REQUIRE_MESSAGE(nh.empty(), "Node handle: node_type object is not empty after default ctor");

    // Move assignment
    // key/mapped/value function
    auto expected_value = *test_table.begin();

    nh = test_table.unsafe_extract(test_table.begin());
    REQUIRE_MESSAGE(!nh.empty(), "Node handle: node_type object is empty after valid move assignment");
    REQUIRE_MESSAGE(nh.get_allocator() == test_table.get_allocator(), "Node handle: node_type object allocator is incorrect");
    REQUIRE_MESSAGE(compare_handle_getters(nh, expected_value),
                    "Node handle: node_type object does not contains expected value after valid move assignment");

    // Move ctor
    node_type nh2(std::move(nh));
    REQUIRE_MESSAGE(nh.empty(), "Node handle: moved-from node_type object is not empty");
    REQUIRE_MESSAGE(!nh2.empty(), "Node handle: node_type object is empty after valid move construction");
    REQUIRE_MESSAGE(compare_handle_getters(nh2, expected_value),
            "Node handle: node_type object does not contains expected value after valid move ctor");

    // bool conversion
    REQUIRE_MESSAGE(nh2, "Node handle: Wrong node handle bool conversion");

    auto expected_value2 = *test_table.begin();
    set_node_handle_value(nh2, expected_value2);
    REQUIRE_MESSAGE(compare_handle_getters(nh2, expected_value2),
                    "Node handle: Wrond node handle key/mapped/value change behaviour");

    // Member and non-member swap check
    node_type empty_node;
    // Extract an element for nh2 and nh3 difference
    test_table.unsafe_extract(test_table.begin());
    auto expected_value3 = *test_table.begin();
    node_type nh3(test_table.unsafe_extract(test_table.begin()));

    // Both of node handles are not empty
    nh3.swap(nh2);
    REQUIRE_MESSAGE(!nh2.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(!nh3.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(nh3, expected_value2),
                    "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(nh2, expected_value3),
                    "Node handle: Wrong node handle swap behavior");

    using std::swap;
    swap(nh2, nh3);
    REQUIRE_MESSAGE(!nh2.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(!nh3.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(nh3, expected_value3),
                    "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(nh2, expected_value2),
                    "Node handle: Wrong node handle swap behavior");

    // One of the handles is empty
    nh3.swap(empty_node);
    REQUIRE_MESSAGE(nh3.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(empty_node, expected_value3),
                    "Node handle: Wrong node handle swap behavior");

    swap(empty_node, nh3);
    REQUIRE_MESSAGE(empty_node.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(nh3, expected_value3),
                    "Node handle: Wrong node handle swap behavior");

    empty_node.swap(nh3);
    REQUIRE_MESSAGE(nh3.empty(), "Node handle: Wrong node handle swap behavior");
    REQUIRE_MESSAGE(compare_handle_getters(empty_node, expected_value3),
                    "Node handle: Wrong node handle swap behavior");
}

template <typename Container>
typename Container::node_type generate_node_handle( const typename Container::value_type& value ) {
    Container table;
    table.insert(value);
    return table.unsafe_extract(table.begin());
}

template <typename Container>
void check_insert( const Container& table,
                   typename Container::iterator result,
                   const typename Container::value_type* node_value = nullptr )
{
    if (node_value == nullptr) {
        REQUIRE_MESSAGE(result == table.end(),
                        "Insert: Result iterator does not point to the end after empty node insertion");
    } else {
        if (AllowMultimapping<Container>::value) {
            REQUIRE_MESSAGE(*result == *node_value,
                    "Insert: Result iterator points to the wrong element after successful insertion");

            for (auto it = table.begin(); it != table.end(); ++it) {
                if (it == result) return;
            }
            REQUIRE_MESSAGE(false, "Insert: iterator does not point to the element in the container");
        } else {
            REQUIRE_MESSAGE((result == table.find(Value<Container>::key(*node_value)) &&
                             result != table.end()),
                             "Insert: Iterator does not point to the equal element in the container");
        }
    }
}

// Overload for sets
template <typename Container>
void check_insert( const Container& table,
                   typename Container::iterator result,
                   bool,
                   const typename Container::value_type* node_value = nullptr )
{
    check_insert(table, result, node_value);
}

// Overload for maps
template <typename Container>
void check_insert( const Container& table,
                   const std::pair<typename Container::iterator, bool>& result,
                   bool successful,
                   const typename Container::value_type* node_value = nullptr )
{
    check_insert(table, result.first, node_value);
    REQUIRE_MESSAGE((result.second == successful || AllowMultimapping<Container>::value),
                    "Insert: Wrong bool returned after node insertion");
}

// Can't delete reference from table_to_insert argument because hint must point to the element in the table
template <typename Container, typename... Hint>
void test_insert_overloads(Container& table_to_insert, const typename Container::value_type& value, const Hint&... hint) {
    // Insert an empty element
    typename Container::node_type nh;

    auto table_size = table_to_insert.size();
    auto result = table_to_insert.insert(hint..., std::move(nh));
    check_insert(table_to_insert, result, /*successful = */false);

    REQUIRE_MESSAGE(table_to_insert.size() == table_size,
                    "Insert: Container size changed after the insertion of the empty node handle");

    // Standard insertion
    nh = generate_node_handle<Container>(value);

    result = table_to_insert.insert(hint..., std::move(nh));
    REQUIRE_MESSAGE(nh.empty(), "Insert: Not empty node handle after successful insertion");
    check_insert(table_to_insert, result, /*successful = */true, &value);

    // Insert existing node
    nh = generate_node_handle<Container>(value);
    result = table_to_insert.insert(hint..., std::move(nh));

    check_insert(table_to_insert, result, /*successful = */false, &value);

    if (AllowMultimapping<Container>::value) {
        REQUIRE_MESSAGE(nh.empty(), "Insert: Failed insertion to multitable");
    } else {
        REQUIRE_MESSAGE(!nh.empty(), "Insert: Empty node handle after failed insertion");
        REQUIRE_MESSAGE(compare_handle_getters(nh, value),
                        "Insert: Existing data does not equal to the one being inserted");
    }
}

template <typename Container>
void test_insert( Container table, const typename Container::value_type& value ) {
    REQUIRE_MESSAGE(!table.empty(), "Insert: container should contains 1 or more elements");
    Container table_backup(table);
    test_insert_overloads(table, value); // test insert
    test_insert_overloads(table_backup, value, table_backup.begin()); // test insert with the hint
}

template <typename Container>
void test_extract( Container table_for_extract, typename Container::key_type new_key ) {
    REQUIRE_MESSAGE(table_for_extract.size() > 1, "Extract: container must contain 2 or more elements");
    REQUIRE_MESSAGE(!table_for_extract.contains(new_key), "Extract: container must not contain new element");

    // Extract new_element
    auto nh = table_for_extract.unsafe_extract(new_key);
    REQUIRE_MESSAGE(nh.empty(), "Extract: node handle is not empty after extraction of key which is not is the container");

    // Valid key extraction
    auto expected_value = *table_for_extract.begin();
    auto key = Value<Container>::key(expected_value);
    auto count = table_for_extract.count(key);

    nh = table_for_extract.unsafe_extract(key);
    REQUIRE_MESSAGE(!nh.empty(), "Extract: node handle is empty after successful extraction");
    REQUIRE_MESSAGE(compare_handle_getters(nh, expected_value), "Extract: node handle contains wrong node after successful extraction");
    REQUIRE_MESSAGE(table_for_extract.count(key) == count -1, "Extract: more than one elements were extracted");

    // Valid iterator extraction
    auto expected_value2 = *table_for_extract.begin();
    auto key2 = Value<Container>::key(expected_value2);
    auto count2 = table_for_extract.count(key2);

    nh = table_for_extract.unsafe_extract(table_for_extract.cbegin());
    REQUIRE_MESSAGE(!nh.empty(), "Extract: node handle is empty after successful extraction");
    REQUIRE_MESSAGE(compare_handle_getters(nh, expected_value2), "Extract: node handle contains wrong node after successful extraction");
    REQUIRE_MESSAGE(table_for_extract.count(key2) == count2 -1, "Extract: more than one elements were extracted");
}

template <typename Container>
void test_node_handling( const Container& container, const typename Container::value_type& new_value ) {
    test_node_handle(container);
    test_insert(container, new_value);
    test_extract(container, Value<Container>::key(new_value));
}

template <typename Container>
void test_node_handling_support() {
    Container cont;

    for (int i = 1; i < 5; ++i) {
        cont.insert(Value<Container>::make(i));
    }

    if (AllowMultimapping<Container>::value) {
        cont.insert(Value<Container>::make(4));
    }

    test_node_handling(cont, Value<Container>::make(5));
}

template <typename Container1, typename Container2>
void test_merge_basic( Container1 table1, Container2&& table2 ) {
    using container2_pure_type = typename std::decay<Container2>::type;

    // Initialization
    Container1 table1_backup = table1;
    container2_pure_type table2_backup = table2;

    table1.merge(std::forward<Container2>(table2));
    for (auto it : table2) {
        REQUIRE_MESSAGE(table1.contains(Value<container2_pure_type>::key(it)),
                        "Merge: Some key was not merged");
    }

    // After the following step table1 will contains only merged elements from table2
    for (auto it : table1_backup) {
        table1.unsafe_extract(Value<Container1>::key(it));
    }
    // After the following step table2_backup will contains only merged elements from table2
    for (auto it : table2) {
        table2_backup.unsafe_extract(Value<container2_pure_type>::key(it));
    }

    REQUIRE_MESSAGE(table1.size() == table2_backup.size(), "Merge: Sizes of tables are not equal");
    for (auto it : table2_backup) {
        REQUIRE_MESSAGE(table1.contains(Value<container2_pure_type>::key(it)),
                        "Merge: Wrong merge behavior");
    }
}

template <typename Container1, typename Container2>
void test_merge_overloads( const Container1& table1, Container2 table2 ) {
    Container2 table_backup(table2);
    test_merge_basic(table1, table2);
    test_merge_basic(table1, std::move(table_backup));
}

template <typename UniqueContainer, typename MultiContainer>
void test_merge_transposition( UniqueContainer table1, UniqueContainer table2,
                               MultiContainer multitable1, MultiContainer multitable2 ) {
    UniqueContainer empty_table;
    MultiContainer empty_multitable;

    // Unique table transpositions
    test_merge_overloads(table1, table2);
    test_merge_overloads(table1, empty_table);
    test_merge_overloads(empty_table, table2);

    // Multi table transpositions
    test_merge_overloads(multitable1, multitable2);
    test_merge_overloads(multitable1, empty_multitable);
    test_merge_overloads(empty_multitable, multitable2);

    // Unique/Multi tables transpositions
    test_merge_overloads(table1, multitable1);
    test_merge_overloads(multitable2, table2);
}

template <typename SrcTableType, typename DstTableType>
void check_concurrent_merge( SrcTableType& start_data, DstTableType& dst_table,
                             std::vector<SrcTableType>& src_tables, std::true_type )
{
    REQUIRE_MESSAGE(dst_table.size() == start_data.size() * src_tables.size(),
                    "Merge: Incorrect merge for some elements");

    for (auto it : start_data) {
        REQUIRE_MESSAGE(dst_table.count(Value<DstTableType>::key(it)) ==
                        start_data.count(Value<SrcTableType>::key(it)) * src_tables.size(),
                        "Merge: Incorrect merge for some elements");
    }

    for (size_t i = 0; i < src_tables.size(); ++i) {
        REQUIRE_MESSAGE(src_tables[i].empty(), "Merge: Some elements were not merged");
    }
}

template <typename SrcTableType, typename DstTableType>
void check_concurrent_merge( SrcTableType& start_data, DstTableType& dst_table,
                             std::vector<SrcTableType>& src_tables, std::false_type )
{
    SrcTableType expected_result;
    for (auto table : src_tables) {
        for (auto it : start_data) {
            // If we cannot find some element in some table, then it has been moved
            if (!table.contains(Value<SrcTableType>::key(it))) {
                bool result = expected_result.insert(it).second;
                REQUIRE_MESSAGE(result,
                                "Merge: Some element was merged twice or was not returned to his owner after unsuccessful merge");
            }
        }
    }

    REQUIRE_MESSAGE((expected_result.size() == dst_table.size() && start_data.size() == dst_table.size()),
                    "Merge: wrong size of result table");

    for (auto it : expected_result) {
        if (dst_table.contains(Value<SrcTableType>::key(it)) &&
            start_data.contains(Value<DstTableType>::key(it)))
        {
            dst_table.unsafe_extract(Value<SrcTableType>::key(it));
            start_data.unsafe_extract(Value<DstTableType>::key(it));
        } else {
            REQUIRE_MESSAGE(false, "Merge: Incorrect merge for some element");
        }
    }

    REQUIRE_MESSAGE((dst_table.empty() && start_data.empty()), "Merge: Some elements were not merged");
}

template <typename SrcTableType, typename DstTableType>
void test_concurrent_merge( SrcTableType table_data ) {
    for (std::size_t num_threads = utils::MinThread; num_threads <= utils::MaxThread; ++num_threads) {
        std::vector<SrcTableType> src_tables;
        DstTableType dst_table;

        for (std::size_t j = 0; j < num_threads; ++j) {
            src_tables.push_back(table_data);
        }

        utils::NativeParallelFor(num_threads, [&](std::size_t index) { dst_table.merge(src_tables[index]); } );

        check_concurrent_merge(table_data, dst_table, src_tables,
                            AllowMultimapping<DstTableType>{});
    }
}

template <typename Container1, typename Container2>
void test_merge( int size ) {
    Container1 table1_1;
    Container1 table1_2;
    int i = 1;

    for (; i < 5; ++i) {
        table1_1.insert(Value<Container1>::make(i));
        table1_2.insert(Value<Container1>::make(i * i));
    }
    if (AllowMultimapping<Container1>::value) {
        table1_1.insert(Value<Container1>::make(i));
        table1_2.insert(Value<Container1>::make(i * i));
    }

    Container2 table2_1;
    Container2 table2_2;

    for (i = 3; i < 7; ++i) {
        table2_1.insert(Value<Container2>::make(i));
        table2_2.insert(Value<Container2>::make(i * i));
    }
    if (AllowMultimapping<Container2>::value) {
        table2_1.insert(Value<Container2>::make(i));
        table2_2.insert(Value<Container2>::make(i * i));
    }

    test_merge_transposition(table1_1, table1_2, table2_1, table2_2);

    Container1 table1_3;
    Container2 table2_3;
    for (i = 0; i < size; ++i) {
        table1_3.insert(Value<Container1>::make(i));
        table2_3.insert(Value<Container2>::make(i));
    }

    test_concurrent_merge<Container1, Container2>(table1_3);
    test_concurrent_merge<Container2, Container1>(table2_3);
}

} // namespace node_handling tests

#endif // __TBB_test_common_node_handling_support_H
