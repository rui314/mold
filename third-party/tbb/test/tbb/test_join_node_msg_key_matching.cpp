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

// Message based key matching is a preview feature
#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "common/config.h"

#include "test_join_node.h"

//! \file test_join_node_msg_key_matching.cpp
//! \brief Test for [preview] functionality

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = MyMessageKeyWithoutKey<int, int>;
    using JoinOutputType = std::tuple<msg_t, msg_t, msg_t>;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = { msg_t(), msg_t(), msg_t() };

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<std::size_t>>, tbb::flow::buffer_node<msg_t>>
        (messages_for_follows);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<std::size_t>>>
        (messages_for_precedes);
}
#endif

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

template <typename T1, typename T2>
using make_tuple = decltype(std::tuple_cat(T1(), std::tuple<T2>()));
using T1 = std::tuple<MyMessageKeyWithoutKeyMethod<std::string, double>>;
using T2 = make_tuple<T1, MyMessageKeyWithBrokenKey<std::string, int>>;
using T3 = make_tuple < T2, MyMessageKeyWithoutKey<std::string, int>>;
using T4 = make_tuple < T3, MyMessageKeyWithoutKeyMethod<std::string, size_t>>;
using T5 = make_tuple < T4, MyMessageKeyWithBrokenKey<std::string, int>>;
using T6 = make_tuple < T5, MyMessageKeyWithoutKeyMethod<std::string, short>>;
using T7 = make_tuple < T6, MyMessageKeyWithoutKeyMethod<std::string, threebyte>>;
using T8 = make_tuple < T7, MyMessageKeyWithBrokenKey<std::string, int>>;
using T9 = make_tuple < T8, MyMessageKeyWithoutKeyMethod<std::string, threebyte>>;
using T10 = make_tuple < T9, MyMessageKeyWithBrokenKey<std::string, size_t>>;

#if TBB_TEST_LOW_WORKLOAD && TBB_USE_DEBUG
// the compiler might generate huge object file in debug (>64M)
#define TEST_CASE_TEMPLATE_N_ARGS(dec) TEST_CASE_TEMPLATE(dec, T, T2, T10)
#else
#define TEST_CASE_TEMPLATE_N_ARGS(dec) TEST_CASE_TEMPLATE(dec, T, T2, T3, T4, T5, T6, T7, T8, T9, T10)
#endif

//! Serial test with different tuple sizes
//! \brief \ref error_guessing
TEST_CASE_TEMPLATE_N_ARGS("Serial N tests") {
    generate_test<serial_test, T, message_based_key_matching<std::string&> >::do_test();
}

//! Parallel test with different tuple sizes
//! \brief \ref error_guessing
TEST_CASE_TEMPLATE_N_ARGS("Parallel N tests") {
    generate_test<parallel_test, T, message_based_key_matching<std::string&> >::do_test();
}

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
