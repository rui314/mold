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

#ifdef TBB_TEST_LOW_WORKLOAD
    #undef MAX_TUPLE_TEST_SIZE
    #define MAX_TUPLE_TEST_SIZE 3
#endif

#include "common/config.h"

#include "test_join_node.h"
#include "common/test_join_node_multiple_predecessors.h"

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
            for(int i = 0; i < N; ++i) all_input_nodes[i][0] = nullptr;

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

// TODO: Look deeper into this test to see if it has the right name
// and if it actually tests some kind of regression. It is possible
// that `connect_join_via_follows` and `connect_join_via_precedes`
// functions are redundant.

//! Test maintaining correct count of ports without input
//! \brief \ref error_guessing
TEST_CASE("Test removal of the predecessor while having none") {
    using namespace multiple_predecessors;

    test(connect_join_via_make_edge);
}
