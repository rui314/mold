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

//! \file conformance_sequencer_node.cpp
//! \brief Test for [flow_graph.sequencer_node] specification

/*
TODO: implement missing conformance tests for sequencer_node:
  - [ ] The copy constructor and copy assignment are called for the node's type template parameter.
  - [ ] Explicit test that `Sequencer' requirements are necessary.
  - [ ] Write tests for the constructors.
  - [ ] Add CTAD test.
  - [ ] Improve `test_buffering' by checking that additional `try_get()' does not receive the same
    value.
  - [ ] Add explicit test on the example from the specification.
*/

template<typename T>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, sequencer_node<T>>::value), "sequencer_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<T>, sequencer_node<T>>::value), "sequencer_node should be derived from receiver<T>");
    CHECK_MESSAGE( (std::is_base_of<sender<T>, sequencer_node<T>>::value), "sequencer_node should be derived from sender<T>");
}

template<typename T>
struct id_sequencer{
    using input_type = T;

    std::size_t operator()(T v) {
        return v;
    }
};

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    id_sequencer<int> sequencer;

    sequencer_node<int> n(g, sequencer);
    sequencer_node<int> n2(n);
}

void test_buffering(){
    oneapi::tbb::flow::graph g;

    id_sequencer<int> sequencer;

    oneapi::tbb::flow::sequencer_node<int> node(g, sequencer);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);
    node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE( (node.try_get(tmp) == false), "try_get after rejection should not succeed");
    CHECK_MESSAGE( (tmp == -1), "try_get after rejection should not set value");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides(){
    // oneapi::tbb::flow::graph g;
    // id_sequencer<int> sequ;
    // oneapi::tbb::flow::sequencer_node node1(g, sequ);
}
#endif

void test_forwarding(){
    oneapi::tbb::flow::graph g;
    id_sequencer<int> sequencer;

    oneapi::tbb::flow::sequencer_node<int> node1(g, sequencer);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(0);

    g.wait_for_all();

    int c2 = get_count(node2), c3 = get_count(node3);
    CHECK_MESSAGE( (c2 != c3 ), "Only one descendant the node needs to receive");
    CHECK_MESSAGE( (c2 + c3 == 1 ), "Messages need to be received");
}

void test_sequencer(){
    oneapi::tbb::flow::graph g;
    id_sequencer<int> sequencer;

    oneapi::tbb::flow::sequencer_node<int> node(g, sequencer);

    node.try_put(1);
    node.try_put(0);
    node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == true), "Getting from sequencer should succeed");
    CHECK_MESSAGE((tmp == 0), "Received value should be correct");

    tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == true), "Getting from sequencer should succeed");
    CHECK_MESSAGE((tmp == 1), "Received value should be correct");

    tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == false), "Getting from sequencer should not succeed");

}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("sequencer_node buffering"){
    test_sequencer();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("sequencer_node buffering"){
    test_forwarding();
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test priority_queue_node buffering
//! \brief \ref requirement
TEST_CASE("sequencer_node buffering"){
    test_buffering();
}

//! Test copy constructor
//! \brief \ref interface
TEST_CASE("sequencer_node copy constructor"){
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("sequencer_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}

