/*
    Copyright (c) 2005-2022 Intel Corporation

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

// Message based key matching is a preview feature
#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "common/config.h"

#include "test_join_node.h"

//! \file test_join_node_msg_key_matching.cpp
//! \brief Test for [preview] functionality

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
struct message_key {
    int my_key;
    double my_value;

    int key() const { return my_key; }

    operator size_t() const { return my_key; }

    bool operator==(const message_key& rhs) { return my_value == rhs.my_value; }
};

void test_deduction_guides() {
    using namespace tbb::flow;
    using tuple_type = std::tuple<message_key, message_key>;

    graph g;
    broadcast_node<message_key> bm1(g), bm2(g);
    broadcast_node<tuple_type> bm3(g);
    join_node<tuple_type, key_matching<int> > j0(g);
    join_node j3(j0);
    static_assert(std::is_same_v<decltype(j3), join_node<tuple_type, key_matching<int>>>);
}
#endif

//! Serial test with matching policies
//! \brief \ref error_guessing
TEST_CASE("Serial test") {
    generate_test<serial_test, std::tuple<MyMessageKeyWithBrokenKey<int, double>, MyMessageKeyWithoutKey<int, float> >, message_based_key_matching<int> >::do_test();
    generate_test<serial_test, std::tuple<MyMessageKeyWithoutKeyMethod<std::string, double>, MyMessageKeyWithBrokenKey<std::string, float> >, message_based_key_matching<std::string> >::do_test();
}

//! Parallel test with special key types
//! \brief \ref error_guessing
TEST_CASE("Parallel test"){
    generate_test<parallel_test, std::tuple<MyMessageKeyWithBrokenKey<int, double>, MyMessageKeyWithoutKey<int, float> >, message_based_key_matching<int> >::do_test();
    generate_test<parallel_test, std::tuple<MyMessageKeyWithoutKeyMethod<int, double>, MyMessageKeyWithBrokenKey<int, float> >, message_based_key_matching<int&> >::do_test();
    generate_test<parallel_test, std::tuple<MyMessageKeyWithoutKey<std::string, double>, MyMessageKeyWithoutKeyMethod<std::string, float> >, message_based_key_matching<std::string&> >::do_test();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("Deduction guides test"){
    test_deduction_guides();
}
#endif
