/*
    Copyright (c) 2020-2021 Intel Corporation

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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "conformance_flowgraph.h"

//! \file conformance_indexer_node.cpp
//! \brief Test for [flow_graph.indexer_node] specification

using input_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/true, /*copy_assign*/false>;
using my_indexer_type = oneapi::tbb::flow::indexer_node<int, float, input_msg>;
using my_output_type = my_indexer_type::output_type;

//! Test node broadcast messages to successors
//! \brief \ref requirement
TEST_CASE("indexer_node broadcasts"){
    oneapi::tbb::flow::graph g;

    my_indexer_type testing_node(g);
    std::vector<conformance::test_push_receiver<my_output_type>*> receiver_nodes;

    for(std::size_t i = 0; i < 3; ++i) {
        receiver_nodes.emplace_back(new conformance::test_push_receiver<my_output_type>(g));
        oneapi::tbb::flow::make_edge(testing_node, *receiver_nodes.back());
    }

    oneapi::tbb::flow::input_port<0>(testing_node).try_put(6);
    oneapi::tbb::flow::input_port<1>(testing_node).try_put(1.5);
    oneapi::tbb::flow::input_port<2>(testing_node).try_put(input_msg(1));
    g.wait_for_all();

    for(auto receiver: receiver_nodes) {
        auto values = conformance::get_values(*receiver);
        CHECK_MESSAGE((values.size() == 3), std::string("Descendant of the node must receive 3 messages."));
        for(auto& value : values){
            if(value.is_a<int>()){
                CHECK_MESSAGE((value.cast_to<int>() == 6), "Value passed is the actual one received.");
            } else if(value.is_a<float>()){
                CHECK_MESSAGE((value.cast_to<float>() == 1.5), "Value passed is the actual one received.");
            } else {
                CHECK_MESSAGE((value.cast_to<input_msg>() == 1), "Value passed is the actual one received.");
            }
        }
        delete receiver;
    }
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("indexer_node superclasses"){
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::graph_node, my_indexer_type>::value), "indexer_node should be derived from graph_node");
}

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("indexer_node buffering") {
    oneapi::tbb::flow::graph g;

    my_indexer_type testing_node(g);

    oneapi::tbb::flow::limiter_node<my_output_type> rejecter(g,0);
    oneapi::tbb::flow::make_edge(testing_node, rejecter);

    oneapi::tbb::flow::input_port<0>(testing_node).try_put(6);
    oneapi::tbb::flow::input_port<1>(testing_node).try_put(1.5);
    oneapi::tbb::flow::input_port<2>(testing_node).try_put(input_msg(1));

    my_output_type tmp;
    CHECK_MESSAGE((testing_node.try_get(tmp) == false), "Value should be discarded after rejection");
    g.wait_for_all();
}

//! Test indexer behaviour
//! \brief \ref requirement
TEST_CASE("indexer_node behaviour") {
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::function_node<int, int> f1( g, oneapi::tbb::flow::unlimited,
                                [](const int &i) { return 2*i; } );
    oneapi::tbb::flow::function_node<float, float> f2( g, oneapi::tbb::flow::unlimited,
                                [](const float &f) { return f/2; } );
    oneapi::tbb::flow::continue_node<input_msg> c1( g,
                                [](oneapi::tbb::flow::continue_msg) { return input_msg(5); } );

    my_indexer_type testing_node(g);

    oneapi::tbb::flow::function_node<my_output_type>
        f3( g, oneapi::tbb::flow::unlimited,
            []( const my_output_type &v ) {
                if (v.tag() == 0) {
                    CHECK_MESSAGE((v.is_a<int>()), "Expected to int" );
                    CHECK_MESSAGE((oneapi::tbb::flow::cast_to<int>(v) == 6), "Expected to receive 6" );
                } else if (v.tag() == 1) {
                    CHECK_MESSAGE((v.is_a<float>()), "Expected to float" );
                    CHECK_MESSAGE((oneapi::tbb::flow::cast_to<float>(v) == 1.5), "Expected to receive 1.5" );
                } else {
                    CHECK_MESSAGE((v.is_a<input_msg>()), "Expected to float" );
                    CHECK_MESSAGE((oneapi::tbb::flow::cast_to<input_msg>(v) == 5), "Expected to receive input_msg(5)" );
                }
            }
        );

    oneapi::tbb::flow::make_edge(f1, oneapi::tbb::flow::input_port<0>(testing_node));
    oneapi::tbb::flow::make_edge(f2, oneapi::tbb::flow::input_port<1>(testing_node));
    oneapi::tbb::flow::make_edge(c1, oneapi::tbb::flow::input_port<2>(testing_node));
    oneapi::tbb::flow::make_edge(testing_node, f3);

    f1.try_put(3);
    f2.try_put(3);
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();
}

//! The node that is constructed has a reference to the same graph object as src.
//! The list of predecessors, messages in the input ports, and successors are not copied.
//! \brief \ref interface
TEST_CASE("indexer_node copy constructor"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::continue_node<int> node0( g,
                                [](oneapi::tbb::flow::continue_msg) { return 1; } );

    my_indexer_type node1(g);
    conformance::test_push_receiver<my_output_type> node2(g);
    conformance::test_push_receiver<my_output_type> node3(g);

    oneapi::tbb::flow::make_edge(node0, oneapi::tbb::flow::input_port<0>(node1));
    oneapi::tbb::flow::make_edge(node1, node2);

    my_indexer_type node_copy(node1);

    oneapi::tbb::flow::make_edge(node_copy, node3);

    oneapi::tbb::flow::input_port<0>(node_copy).try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor");

    node0.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor");
}

//! Test indexer_node output_type
//! \brief \ref interface \ref requirement
TEST_CASE("indexer_node output_type") {
    CHECK_MESSAGE((conformance::check_output_type<my_output_type, oneapi::tbb::flow::tagged_msg<size_t, int, float, input_msg>>()), "indexer_node output_type should returns a tagged_msg");
}
