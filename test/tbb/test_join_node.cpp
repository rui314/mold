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

#ifdef TBB_TEST_LOW_WORKLOAD
    #undef MAX_TUPLE_TEST_SIZE
    #define MAX_TUPLE_TEST_SIZE 3
#endif

#include "common/config.h"

#include "test_join_node.h"

//! \file test_join_node.cpp
//! \brief Test for [flow_graph.join_node] specification


static std::atomic<int> output_count;

// get the tag from the output tuple and emit it.
// the first tuple component is tag * 2 cast to the type
template<typename OutputTupleType>
class recirc_output_func_body {
public:
    // we only need this to use input_node_helper
    typedef typename tbb::flow::join_node<OutputTupleType, tbb::flow::tag_matching> join_node_type;
    static const int N = std::tuple_size<OutputTupleType>::value;
    int operator()(const OutputTupleType &v) {
        int out = int(std::get<0>(v))/2;
        input_node_helper<N, join_node_type>::only_check_value(out, v);
        ++output_count;
        return out;
    }
};

template<typename JType>
class tag_recirculation_test {
public:
    typedef typename JType::output_type TType;
    typedef typename std::tuple<int, tbb::flow::continue_msg> input_tuple_type;
    typedef tbb::flow::join_node<input_tuple_type, tbb::flow::reserving> input_join_type;
    static const int N = std::tuple_size<TType>::value;
    static void test() {
        input_node_helper<N, JType>::print_remark("Recirculation test of tag-matching join");
        INFO(" >\n");
        for(int maxTag = 1; maxTag <10; maxTag *= 3) {
            for(int i = 0; i < N; ++i) all_input_nodes[i][0] = NULL;

            tbb::flow::graph g;
            // this is the tag-matching join we're testing
            JType * my_join = makeJoin<N, JType, tbb::flow::tag_matching>::create(g);
            // input_node for continue messages
            tbb::flow::input_node<tbb::flow::continue_msg> snode(g, recirc_input_node_body());
            // reserving join that matches recirculating tags with continue messages.
            input_join_type * my_input_join = makeJoin<2, input_join_type, tbb::flow::reserving>::create(g);
            // tbb::flow::make_edge(snode, tbb::flow::input_port<1>(*my_input_join));
            tbb::flow::make_edge(snode, std::get<1>(my_input_join->input_ports()));
            // queue to hold the tags
            tbb::flow::queue_node<int> tag_queue(g);
            tbb::flow::make_edge(tag_queue, tbb::flow::input_port<0>(*my_input_join));
            // add all the function_nodes that are inputs to the tag-matching join
            input_node_helper<N, JType>::add_recirc_func_nodes(*my_join, *my_input_join, g);
            // add the function_node that accepts the output of the join and emits the int tag it was based on
            tbb::flow::function_node<TType, int> recreate_tag(g, tbb::flow::unlimited, recirc_output_func_body<TType>());
            tbb::flow::make_edge(*my_join, recreate_tag);
            // now the recirculating part (output back to the queue)
            tbb::flow::make_edge(recreate_tag, tag_queue);

            // put the tags into the queue
            for(int t = 1; t<=maxTag; ++t) tag_queue.try_put(t);

            input_count = Recirc_count;
            output_count = 0;

            // start up the source node to get things going
            snode.activate();

            // wait for everything to stop
            g.wait_for_all();

            CHECK_MESSAGE( (output_count==Recirc_count), "not all instances were received");

            int j{};
            // grab the tags from the queue, record them
            std::vector<bool> out_tally(maxTag, false);
            for(int i = 0; i < maxTag; ++i) {
                CHECK_MESSAGE( (tag_queue.try_get(j)), "not enough tags in queue");
                CHECK_MESSAGE( (!out_tally.at(j-1)), "duplicate tag from queue");
                out_tally[j-1] = true;
            }
            CHECK_MESSAGE( (!tag_queue.try_get(j)), "Extra tags in recirculation queue");

            // deconstruct graph
            input_node_helper<N, JType>::remove_recirc_func_nodes(*my_join, *my_input_join);
            tbb::flow::remove_edge(*my_join, recreate_tag);
            makeJoin<N, JType, tbb::flow::tag_matching>::destroy(my_join);
            tbb::flow::remove_edge(tag_queue, tbb::flow::input_port<0>(*my_input_join));
            tbb::flow::remove_edge(snode, tbb::flow::input_port<1>(*my_input_join));
            makeJoin<2, input_join_type, tbb::flow::reserving>::destroy(my_input_join);
        }
    }
};

template<typename JType>
class generate_recirc_test {
public:
    typedef tbb::flow::join_node<JType, tbb::flow::tag_matching> join_node_type;
    static void do_test() {
        tag_recirculation_test<join_node_type>::test();
    }
};

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
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
#endif

namespace multiple_predecessors {

using namespace tbb::flow;

using join_node_t = join_node<std::tuple<continue_msg, continue_msg, continue_msg>, reserving>;
using queue_node_t = queue_node<std::tuple<continue_msg, continue_msg, continue_msg>>;

void twist_join_connections(
    buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2, buffer_node<continue_msg>& bn3,
    join_node_t& jn)
{
    // order, in which edges are created/destroyed, is important
    make_edge(bn1, input_port<0>(jn));
    make_edge(bn2, input_port<0>(jn));
    make_edge(bn3, input_port<0>(jn));

    remove_edge(bn3, input_port<0>(jn));
    make_edge  (bn3, input_port<2>(jn));

    remove_edge(bn2, input_port<0>(jn));
    make_edge  (bn2, input_port<1>(jn));
}

std::unique_ptr<join_node_t> connect_join_via_make_edge(
    graph& g, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    std::unique_ptr<join_node_t> jn( new join_node_t(g) );
    twist_join_connections( bn1, bn2, bn3, *jn );
    make_edge(*jn, qn);
    return jn;
}

#if TBB_PREVIEW_FLOW_GRAPH_FEATURES
std::unique_ptr<join_node_t> connect_join_via_follows(
    graph&, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    auto bn_set = make_node_set(bn1, bn2, bn3);
    std::unique_ptr<join_node_t> jn( new join_node_t(follows(bn_set)) );
    make_edge(*jn, qn);
    return jn;
}

std::unique_ptr<join_node_t> connect_join_via_precedes(
    graph&, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    auto qn_set = make_node_set(qn);
    auto qn_copy_set = qn_set;
    std::unique_ptr<join_node_t> jn( new join_node_t(precedes(qn_copy_set)) );
    twist_join_connections( bn1, bn2, bn3, *jn );
    return jn;
}
#endif // TBB_PREVIEW_FLOW_GRAPH_FEATURES

void run_and_check(
    graph& g, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn, bool expected)
{
    std::tuple<continue_msg, continue_msg, continue_msg> msg;

    bn1.try_put(continue_msg());
    bn2.try_put(continue_msg());
    bn3.try_put(continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE(
        (qn.try_get(msg) == expected),
        "Unexpected message absence/existence at the end of the graph."
    );
}

template<typename ConnectJoinNodeFunc>
void test(ConnectJoinNodeFunc&& connect_join_node) {
    graph g;
    buffer_node<continue_msg> bn1(g);
    buffer_node<continue_msg> bn2(g);
    buffer_node<continue_msg> bn3(g);
    queue_node_t qn(g);

    auto jn = connect_join_node(g, bn1, bn2, bn3, qn);

    run_and_check(g, bn1, bn2, bn3, qn, /*expected=*/true);

    remove_edge(bn3, input_port<2>(*jn));
    remove_edge(bn2, input_port<1>(*jn));
    remove_edge(bn1, *jn); //Removes an edge between a sender and port 0 of a multi-input successor.
    remove_edge(*jn, qn);

    run_and_check(g, bn1, bn2, bn3, qn, /*expected=*/false);
}
} // namespace multiple_predecessors


#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and preceedes API"){
    test_follows_and_precedes_api();
}
#endif

//! Test hash buffers behavior
//! \brief \ref error_guessing
TEST_CASE("Tagged buffers test"){
    TestTaggedBuffers();
}

//! Test with various policies and tuple sizes
//! \brief \ref error_guessing
TEST_CASE("Main test"){
    test_main<tbb::flow::queueing>();
    test_main<tbb::flow::reserving>();
    test_main<tbb::flow::tag_matching>();
}

//! Test with recirculating tags
//! \brief \ref error_guessing
TEST_CASE("Recirculation test"){
    generate_recirc_test<std::tuple<int,float> >::do_test();
}

//! Test maintaining correct count of ports without input
//! \brief \ref error_guessing
TEST_CASE("Test removal of the predecessor while having none") {
    using namespace multiple_predecessors;

    test(connect_join_via_make_edge);
#if TBB_PREVIEW_FLOW_GRAPH_FEATURES
    test(connect_join_via_follows);
    test(connect_join_via_precedes);
#endif
}
