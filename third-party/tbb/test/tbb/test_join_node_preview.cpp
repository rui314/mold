/*
    Copyright (c) 2023 Intel Corporation

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

#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "common/config.h"

#include "test_join_node.h"
#include "common/test_join_node_multiple_predecessors.h"
#include "common/test_follows_and_precedes_api.h"

#include <array>
#include <vector>

//! \file test_join_node_preview.cpp
//! \brief Test for [preview] functionality

void jn_follows_and_precedes() {
    using msg_t = tbb::flow::continue_msg;
    using JoinOutputType = std::tuple<msg_t, msg_t, msg_t>;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t(), msg_t(), msg_t()};

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType>, tbb::flow::buffer_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::queueing>>(messages_for_follows);
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::reserving>, tbb::flow::buffer_node<msg_t>>(messages_for_follows);
    auto b = [](msg_t) { return msg_t(); };
    class hash_compare {
    public:
        std::size_t hash(msg_t) const { return 0; }
        bool equal(msg_t, msg_t) const { return true; }
    };
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<msg_t, hash_compare>>, tbb::flow::buffer_node<msg_t>>
        (messages_for_follows, b, b, b);

    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::queueing>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::reserving>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<msg_t, hash_compare>>>
        (messages_for_precedes, b, b, b);
}

void jn_msg_key_matching_follows_and_precedes() {
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

void test_follows_and_precedes_api() {
    jn_follows_and_precedes();
    jn_msg_key_matching_follows_and_precedes();
}

//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and precedes API"){
    test_follows_and_precedes_api();
}

// TODO: Look deeper into this test to see if it has the right name
// and if it actually tests some kind of regression. It is possible
// that `connect_join_via_follows` and `connect_join_via_precedes`
// functions are redundant.

//! Test maintaining correct count of ports without input
//! \brief \ref error_guessing
TEST_CASE("Test removal of the predecessor while having none") {
    using namespace multiple_predecessors;

    test(connect_join_via_follows);
    test(connect_join_via_precedes);
}
