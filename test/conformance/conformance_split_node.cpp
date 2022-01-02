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

//! \file conformance_split_node.cpp
//! \brief Test for [flow_graph.split_node] specification

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true, /*copy_assign*/true/*enable for queue_node successor*/>;
using my_input_tuple = std::tuple<int, float, input_msg>;
using my_split_type = oneapi::tbb::flow::split_node<my_input_tuple>;

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("split_node buffering") {
    oneapi::tbb::flow::graph g;

    my_split_type testing_node(g);

    oneapi::tbb::flow::limiter_node<int> rejecter1(g,0);
    oneapi::tbb::flow::limiter_node<float> rejecter2(g,0);
    oneapi::tbb::flow::limiter_node<input_msg> rejecter3(g,0);

    oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<0>(testing_node), rejecter1);
    oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<1>(testing_node), rejecter2);
    oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<2>(testing_node), rejecter3);

    my_input_tuple my_tuple(1, 1.5f, input_msg(2));
    testing_node.try_put(my_tuple);
    g.wait_for_all();

    int tmp1 = -1;
    float tmp2 = -1;
    input_msg tmp3(-1);
    CHECK_MESSAGE((oneapi::tbb::flow::output_port<0>(testing_node).try_get(tmp1) == false 
                    && tmp1 == -1), "Value should be discarded after rejection");
    CHECK_MESSAGE((oneapi::tbb::flow::output_port<1>(testing_node).try_get(tmp2) == false 
                    && tmp2 == -1.f), "Value should be discarded after rejection");
    CHECK_MESSAGE((oneapi::tbb::flow::output_port<2>(testing_node).try_get(tmp3) == false 
                    && tmp3 == -1), "Value should be discarded after rejection");
}

//! Test node broadcast messages to successors and splitting them in correct order
//! \brief \ref requirement
TEST_CASE("split_node broadcast and splitting"){
    using namespace oneapi::tbb::flow;
    oneapi::tbb::flow::graph g;

    my_split_type testing_node(g);
    conformance::test_push_receiver<int> node2(g);
    conformance::test_push_receiver<float> node3(g);
    conformance::test_push_receiver<input_msg> node4(g);

    oneapi::tbb::flow::make_edge(output_port<0>(testing_node), node2);
    oneapi::tbb::flow::make_edge(output_port<1>(testing_node), node3);
    oneapi::tbb::flow::make_edge(output_port<2>(testing_node), node4);

    my_input_tuple my_tuple(1, 1.5f, input_msg(2));

    CHECK_MESSAGE((testing_node.try_put(my_tuple)), "`try_put()' must always returns `true'");
    g.wait_for_all();
    auto values1 = conformance::get_values(node2);
    auto values2 = conformance::get_values(node3);
    auto values3 = conformance::get_values(node4);

    CHECK_MESSAGE((values1.size() == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE((values2.size() == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE((values3.size() == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE((values1[0] == 1), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE((values2[0] == 1.5f), "Descendant of the node must receive one message.");
    CHECK_MESSAGE((values3[0] == 2), "Descendant of the node must receive one message.");
}

//! The node that is constructed has a reference to the same graph object as src.
//! The predecessors and successors of src are not copied.
//! \brief \ref interface
TEST_CASE("split_node copy constructor"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::continue_node<std::tuple<int>> node0( g,
                                [](oneapi::tbb::flow::continue_msg) { return std::tuple<int>(1); } );

    oneapi::tbb::flow::split_node<std::tuple<int>> node1(g);
    conformance::test_push_receiver<int> node2(g);
    conformance::test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node0, node1);
    oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<0>(node1), node2);

    oneapi::tbb::flow::split_node<std::tuple<int>> node_copy(node1);

    oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<0>(node_copy), node3);

    node_copy.try_put(std::tuple<int>(1));
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor");

    node0.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor");
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("split_node superclasses") {
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::graph_node, my_split_type>::value), "split_node should be derived from graph_node");
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::receiver<my_input_tuple>, my_split_type>::value), "split_node should be derived from receiver<T>");
}

//! Test split_node output_ports() returns a tuple of output ports.
//! \brief \ref interface \ref requirement
TEST_CASE("split_node output_ports") {
    oneapi::tbb::flow::graph g;
    my_split_type node(g);

    CHECK_MESSAGE((std::is_same<my_split_type::output_ports_type&,
        decltype(node.output_ports())>::value), "split_node output_ports should returns a tuple of output ports");
}
