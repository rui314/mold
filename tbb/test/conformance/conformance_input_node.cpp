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

#include "conformance_flowgraph.h"

//! \file conformance_input_node.cpp
//! \brief Test for [flow_graph.input_node] specification

/*
TODO: implement missing conformance tests for input_node:
  - [ ] The `copy_body' function copies altered body (e.g. after its successful invocation).
  - [ ] Check that in `test_forwarding' the value passed is the actual one received.
  - [ ] Improve CTAD test to assert result node type.
  - [ ] Explicit test for copy constructor of the node.
  - [ ] Check `Output' type indeed copy-constructed and copy-assigned while working with the node.
  - [ ] Check node cannot have predecessors (Will argument-dependent lookup be of any help here?)
  - [ ] Check the node is serial and its body never invoked concurrently.
  - [ ] `try_get()' call testing: a call to body is made only when the internal buffer is empty.
*/

std::atomic<size_t> global_execute_count;

template<typename OutputType>
struct input_functor {
    const size_t n;

    input_functor( ) : n(10) { }
    input_functor( const input_functor &f ) : n(f.n) {  }
    void operator=(const input_functor &f) { n = f.n; }

    OutputType operator()( oneapi::tbb::flow_control & fc ) {
       ++global_execute_count;
       if(global_execute_count > n){
           fc.stop();
           return OutputType();
       }
       return OutputType(global_execute_count.load());
    }

};

template<typename O>
struct CopyCounterBody{
    size_t copy_count;

    CopyCounterBody():
        copy_count(0) {}

    CopyCounterBody(const CopyCounterBody<O>& other):
        copy_count(other.copy_count + 1) {}

    CopyCounterBody& operator=(const CopyCounterBody<O>& other) {
        copy_count = other.copy_count + 1; return *this;
    }

    O operator()(oneapi::tbb::flow_control & fc){
        fc.stop();
        return O();
    }
};


void test_input_body(){
    oneapi::tbb::flow::graph g;
    input_functor<int> fun;

    global_execute_count = 0;
    oneapi::tbb::flow::input_node<int> node1(g, fun);
    test_push_receiver<int> node2(g);

    oneapi::tbb::flow::make_edge(node1, node2);

    node1.activate();
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 10), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (global_execute_count == 10 + 1), "Body of the node needs to be executed N + 1 times");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides(){
    oneapi::tbb::flow::graph g;
    input_functor<int> fun;
    oneapi::tbb::flow::input_node node1(g, fun);
}
#endif

void test_buffering(){
    oneapi::tbb::flow::graph g;
    input_functor<int> fun;
    global_execute_count = 0;

    oneapi::tbb::flow::input_node<int> source(g, fun);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(source, rejecter);
    source.activate();
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE( (source.try_get(tmp) == true), "try_get after rejection should succeed");
    CHECK_MESSAGE( (tmp == 1), "try_get should return correct value");
}

void test_forwarding(){
    oneapi::tbb::flow::graph g;
    input_functor<int> fun;

    global_execute_count = 0;
    oneapi::tbb::flow::input_node<int> node1(g, fun);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.activate();
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 10), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (get_count(node3) == 10), "Descendant of the node needs to be receive N messages");
}

template<typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, input_node<O>>::value), "input_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<sender<O>, input_node<O>>::value), "input_node should be derived from sender<Output>");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    CopyCounterBody<int> b;

    graph g;
    input_node<int> fn(g, b);

    CopyCounterBody<int> b2 = copy_body<CopyCounterBody<int>, input_node<int>>(fn);

    CHECK_MESSAGE( (b.copy_count + 2 <= b2.copy_count), "copy_body and constructor should copy bodies");
}

//! Test body copying and copy_body logic
//! \brief \ref interface
TEST_CASE("input_node and body copying"){
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("input_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}

//! Test input_node forwarding
//! \brief \ref requirement
TEST_CASE("input_node forwarding"){
    test_forwarding();
}

//! Test input_node buffering
//! \brief \ref requirement
TEST_CASE("input_node buffering"){
    test_buffering();
}

//! Test calling input_node body
//! \brief \ref interface \ref requirement
TEST_CASE("input_node body") {
    test_input_body();
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}
