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

#include "common/test.h"

#include "common/utils.h"
#include "common/graph_utils.h"

#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/global_control.h"

#include "conformance_flowgraph.h"

//! \file conformance_broadcast_node.cpp
//! \brief Test for [flow_graph.broadcast_node] specification

/*
TODO: implement missing conformance tests for broadcast_node:
  - [ ] The copy constructor and copy assignment are called for the node's type template parameter.
  - [ ] Improve test for constructors.
*/

template<typename T>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, broadcast_node<T>>::value), "broadcast_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<T>, broadcast_node<T>>::value), "broadcast_node should be derived from receiver<T>");
    CHECK_MESSAGE( (std::is_base_of<sender<T>, broadcast_node<T>>::value), "broadcast_node should be derived from sender<T>");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    broadcast_node<int> n(g);
    broadcast_node<int> n2(n);
}

void test_buffering(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::broadcast_node<int> node(g);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);

    node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE( (node.try_get(tmp) == false), "try_get after rejection should not succeed");
    CHECK_MESSAGE( (tmp == -1), "try_get after rejection should not set value");
}

void test_forwarding(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::broadcast_node<int> node1(g);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(1);
    g.wait_for_all();

    int c2 = get_count(node2), c3 = get_count(node3);
    CHECK_MESSAGE( ( c2 == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE( ( c3 == 1), "Descendant of the node must receive one message.");
}

//! Test function_node broadcast
//! \brief \ref requirement
TEST_CASE("broadcast_node broadcasts"){
    test_forwarding();
}

//! Test broadcast_node buffering
//! \brief \ref requirement
TEST_CASE("broadcast_node buffering"){
    test_buffering();
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("broadcast_node copy constructor"){
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("broadcast_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}

