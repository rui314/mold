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

//! \file conformance_overwrite_node.cpp
//! \brief Test for [flow_graph.overwrite_node] specification

/*
TODO: implement missing conformance tests for overwrite_node:
  - [ ] The copy constructor and copy assignment are called for the node's type template parameter.
  - [ ] Improve copy constructor test and general constructor tests.
  - [ ] Write test checking the value is initially invalid.
  - [ ] Write test checking that the gets from the node are non-destructive, but the first `try_get'
    fails.
  - [ ] Test that first `try_get' fails.
  - [ ] Add test on `overwrite_node::is_valid()' method.
  - [ ] Add test on `overwrite_node::clear()' method.
  - [ ] Add test with reserving `join_node' as node's successor. Use example from the spec.
*/

template<typename T>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, overwrite_node<T>>::value), "overwrite_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<T>, overwrite_node<T>>::value), "overwrite_node should be derived from receiver<T>");
    CHECK_MESSAGE( (std::is_base_of<sender<T>, overwrite_node<T>>::value), "overwrite_node should be derived from sender<T>");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    overwrite_node<int> fn(g);
    overwrite_node<int> fn2(fn);

}

void test_buffering(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::overwrite_node<int> node(g);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);
    node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE( (node.try_get(tmp) == true), "try_get after rejection should succeed");
    CHECK_MESSAGE( (tmp == 1), "try_get after rejection should set value");
}

void test_forwarding(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::overwrite_node<int> node1(g);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

void test_overwriting(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::overwrite_node<int> node1(g);

    int tmp = -1;
    node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (node1.try_get(tmp) == true), "Descendant needs to receive a message");
    CHECK_MESSAGE( (tmp == 1), "Descendant needs to receive a correct value");

    node1.try_put(2);
    g.wait_for_all();

    CHECK_MESSAGE( (node1.try_get(tmp) == true), "Descendant needs to receive a message");
    CHECK_MESSAGE( (tmp == 2), "Descendant needs to receive a correct value");
}

//! Test overwrite_node behavior
//! \brief \ref requirement
TEST_CASE("overwrite_node messages"){
    test_overwriting();
}

//! Test function_node broadcast
//! \brief \ref requirement
TEST_CASE("overwrite_node broadcast"){
    test_forwarding();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("overwrite_node buffering"){
    test_buffering();
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("overwrite_node copy constructor"){
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("overwrite_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}
