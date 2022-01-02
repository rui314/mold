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

//! \file conformance_broadcast_node.cpp
//! \brief Test for [flow_graph.broadcast_node] specification

using input_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/true/*enable for queue_node successor*/, /*copy_assign*/true/*enable for queue_node successor*/>;

//! Test function_node broadcast
//! \brief \ref requirement
TEST_CASE("broadcast_node broadcasts"){
    conformance::test_forwarding<oneapi::tbb::flow::broadcast_node<int>, int>(1);
    conformance::test_forwarding<oneapi::tbb::flow::broadcast_node<input_msg>, input_msg>(1);
}

//! Test broadcast_node buffering
//! \brief \ref requirement
TEST_CASE("broadcast_node buffering"){
    conformance::test_buffering<oneapi::tbb::flow::broadcast_node<int>, int>();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("broadcast_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::broadcast_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::broadcast_node<float>, float, float>();
    conformance::test_inheritance<oneapi::tbb::flow::broadcast_node<input_msg>, input_msg, input_msg>();
}

//! The node that is constructed has a reference to the same graph object as src.
//! The predecessors and successors of src are not copied.
//! \brief \ref interface
TEST_CASE("broadcast_node copy constructor"){
    using namespace oneapi::tbb::flow;
    graph g;

    broadcast_node<int> node0(g);
    broadcast_node<int> node1(g);
    conformance::test_push_receiver<int> node2(g);
    conformance::test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node0, node1);
    oneapi::tbb::flow::make_edge(node1, node2);

    broadcast_node<int> node_copy(node1);

    oneapi::tbb::flow::make_edge(node_copy, node3);

    node_copy.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor");

    node0.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor");
}
