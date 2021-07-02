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

//! \file conformance_limiter_node.cpp
//! \brief Test for [flow_graph.limiter_node] specification

/*
TODO: implement missing conformance tests for limiter_node:
  - [ ] The copy constructor and copy assignment are called for the node's type template parameter.
  - [ ] Add use of `decrement' member into the `test_limiting' and see how positive and negative
    values sent to `decrement's' port affect node's internal threshold.
  - [ ] Add test checking the node gets value from the predecessors when threshold decreases enough.
  - [ ] Add test that `continue_msg' decreases the threshold by one.
*/

template<typename T>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, limiter_node<T>>::value), "sequencer_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<T>, limiter_node<T>>::value), "sequencer_node should be derived from receiver<T>");
    CHECK_MESSAGE( (std::is_base_of<sender<T>, limiter_node<T>>::value), "sequencer_node should be derived from sender<T>");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    limiter_node<int> n(g, 5);
    limiter_node<int> n2(n);

}

void test_buffering(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::limiter_node<int> node(g, 5);
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

    oneapi::tbb::flow::limiter_node<int> node1(g, 5);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

void test_limiting(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::limiter_node<int> node1(g, 5);
    test_push_receiver<int> node2(g);

    oneapi::tbb::flow::make_edge(node1, node2);

    for(int i = 0; i < 10; ++i)
        node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 5), "Descendant of the node needs be receive limited number of messages");
}

//! Test limiter_node limiting
//! \brief \ref requirement
TEST_CASE("limiter_node limiting"){
    test_limiting();
}

//! Test function_node broadcast
//! \brief \ref requirement
TEST_CASE("limiter_node broadcast"){
    test_forwarding();
}

//! Test limiter_node buffering
//! \brief \ref requirement
TEST_CASE("limiter_node buffering"){
    test_buffering();
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("limiter_node copy constructor"){
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("limiter_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}

