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

//! \file conformance_split_node.cpp
//! \brief Test for [flow_graph.split_node] specification

/*
TODO: implement missing conformance tests for split_node:
  - [ ] Check that copy constructor and copy assignment is called for each type the tuple stores.
  - [ ] Rewrite `test_forwarding' to check broadcast semantics of the node.
  - [ ] Improve test for constructors.
  - [ ] Unify code style in the test by extracting the implementation from the `TEST_CASE' scope
    into separate functions.
  - [ ] Rename discarding test to `test_buffering' and add checking that the value does not change
    in the `try_get()' method of the output ports of the node.
  - [ ] Add checking of the unlimited concurrency.
  - [ ] Check that `try_put()' always returns `true'.
  - [ ] Explicitly check that `output_ports_type' is defined, accessible.
  - [ ] Explicitly check the method `indexer_node::output_ports()' exists, is accessible and it
    returns a reference to the `output_ports_type' type.
*/

using namespace oneapi::tbb::flow;
using namespace std;

template<typename T>
void test_inheritance(){
    CHECK_MESSAGE( (std::is_base_of<graph_node, split_node<std::tuple<T,T>>>::value), "split_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<std::tuple<T,T>>, split_node<std::tuple<T,T>>>::value), "split_node should be derived from receiver<T>");
}

void test_split(){
    graph g;

    queue_node<int> first_queue(g);
    queue_node<int> second_queue(g);
    split_node< std::tuple<int,int> > my_split_node(g);
    make_edge(output_port<0>(my_split_node), first_queue);
    make_edge(output_port<1>(my_split_node), second_queue);

    tuple<int, int> my_tuple(0, 1);
    my_split_node.try_put(my_tuple);

    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((first_queue.try_get(tmp) == true), "Getting from target queue should succeed");
    CHECK_MESSAGE((tmp == 0), "Received value should be correct");

    tmp = -1;
    CHECK_MESSAGE((second_queue.try_get(tmp) == true), "Getting from target queue should succeed");
    CHECK_MESSAGE((tmp == 1), "Received value should be correct");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    split_node<std::tuple<int, int>> n(g);
    split_node<std::tuple<int, int>> n2(n);
}

void test_forwarding(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::split_node<std::tuple<int, int>> node1(g);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(output_port<0>(node1), node2);
    oneapi::tbb::flow::make_edge(output_port<1>(node1), node3);

    tuple<int, int> my_tuple(0, 1);
    node1.try_put(my_tuple);

    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

//! Test broadcast
//! \brief \ref interface
TEST_CASE("split_node broadcast") {
    test_forwarding();
}

//! Test discarding property
//! \brief \ref requirement
TEST_CASE("split_node discarding") {
    graph g;

    split_node< std::tuple<int,int> > my_split_node(g);

    limiter_node< int > rejecter1( g,0);
    limiter_node< int > rejecter2( g,0);

    make_edge(output_port<0>(my_split_node), rejecter2);
    make_edge(output_port<1>(my_split_node), rejecter1);

    tuple<int, int> my_tuple(0, 1);
    my_split_node.try_put(my_tuple);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((output_port<0>(my_split_node).try_get(tmp) == false), "Value should be discarded after rejection");
    CHECK_MESSAGE((output_port<1>(my_split_node).try_get(tmp) == false), "Value should be discarded after rejection");
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("split_node copy constructor") {
    test_copies();
}

//! Test copy constructor
//! \brief \ref interface \ref requirement
TEST_CASE("split_node messages") {
    test_split();
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("split_node superclasses") {
    test_inheritance<int>();
}

