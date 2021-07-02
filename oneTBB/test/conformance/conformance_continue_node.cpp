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

//! \file conformance_continue_node.cpp
//! \brief Test for [flow_graph.continue_node] specification

/*
TODO: implement missing conformance tests for continue_node:
  - [ ] For `test_forwarding' check that the value passed is the actual one received.
  - [ ] The `copy_body' function copies altered body (e.g. after its successful invocation).
  - [ ] Improve CTAD test.
  - [ ] Improve constructors test, including addition of calls to constructors with
    `number_of_predecessors' parameter.
  - [ ] Explicit test for copy constructor of the node.
  - [ ] Rewrite test_priority.
  - [ ] Check `Output' type indeed copy-constructed and copy-assigned while working with the node.
  - [ ] Explicit test for correct working of `number_of_predecessors' constructor parameter,
    including taking it into account when making and removing edges.
  - [ ] Add testing of `try_put' statement. In particular that it does not wait for the execution of
    the body to complete.
*/

void test_cont_body(){
    oneapi::tbb::flow::graph g;
    inc_functor<int> cf;
    cf.execute_count = 0;

    oneapi::tbb::flow::continue_node<int> node1(g, cf);

    const size_t n = 10;
    for(size_t i = 0; i < n; ++i) {
        CHECK_MESSAGE((node1.try_put(oneapi::tbb::flow::continue_msg()) == true),
                      "continue_node::try_put() should never reject a message.");
    }
    g.wait_for_all();

    CHECK_MESSAGE( (cf.execute_count == n), "Body of the first node needs to be executed N times");
}

template<typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, continue_node<O>>::value), "continue_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<continue_msg>, continue_node<O>>::value), "continue_node should be derived from receiver<Input>");
    CHECK_MESSAGE( (std::is_base_of<sender<O>, continue_node<O>>::value), "continue_node should be derived from sender<Output>");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides(){
    oneapi::tbb::flow::graph g;
    inc_functor<int> fun;
    oneapi::tbb::flow::continue_node node1(g, fun);
}
#endif

void test_forwarding(){
    oneapi::tbb::flow::graph g;
    inc_functor<int> fun;
    fun.execute_count = 0;
    
    oneapi::tbb::flow::continue_node<int> node1(g, fun);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

void test_buffering(){
    oneapi::tbb::flow::graph g;
    inc_functor<int> fun;

    oneapi::tbb::flow::continue_node<int> node(g, fun);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);
    node.try_put(oneapi::tbb::flow::continue_msg());

    int tmp = -1;
    CHECK_MESSAGE( (node.try_get(tmp) == false), "try_get after rejection should not succeed");
    CHECK_MESSAGE( (tmp == -1), "try_get after rejection should not alter passed value");
    g.wait_for_all();
}

void test_policy_ctors(){
    using namespace oneapi::tbb::flow;
    graph g;

    inc_functor<int> fun;

    continue_node<int, lightweight> lw_node(g, fun);
}

void test_ctors(){
    using namespace oneapi::tbb::flow;
    graph g;

    inc_functor<int> fun;

    continue_node<int> proto1(g, 2, fun, oneapi::tbb::flow::node_priority_t(1));
}

template<typename O>
struct CopyCounterBody{
    size_t copy_count;

    CopyCounterBody():
        copy_count(0) {}

    CopyCounterBody(const CopyCounterBody<O>& other):
        copy_count(other.copy_count + 1) {}

    CopyCounterBody& operator=(const CopyCounterBody<O>& other){
        copy_count = other.copy_count + 1;
        return *this;
    }

    O operator()(oneapi::tbb::flow::continue_msg){
        return 1;
    }
};

void test_copies(){
    using namespace oneapi::tbb::flow;

    CopyCounterBody<int> b;

    graph g;
    continue_node<int> fn(g, b);

    CopyCounterBody<int> b2 = copy_body<CopyCounterBody<int>,
                                             continue_node<int>>(fn);

    CHECK_MESSAGE( (b.copy_count + 2 <= b2.copy_count), "copy_body and constructor should copy bodies");
}


void test_priority(){
    size_t concurrency_limit = 1;
    oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_limit);

    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg> source(g,
                                                             [](oneapi::tbb::flow::continue_msg){ return oneapi::tbb::flow::continue_msg();});
    source.try_put(oneapi::tbb::flow::continue_msg());

    first_functor<int>::first_id = -1;
    first_functor<int> low_functor(1);
    first_functor<int> high_functor(2);

    oneapi::tbb::flow::continue_node<int, int> high(g, high_functor, oneapi::tbb::flow::node_priority_t(1));
    oneapi::tbb::flow::continue_node<int, int> low(g, low_functor);

    make_edge(source, low);
    make_edge(source, high);

    g.wait_for_all();

    CHECK_MESSAGE( (first_functor<int>::first_id == 2), "High priority node should execute first");
}

//! Test node costructors
//! \brief \ref requirement
TEST_CASE("continue_node constructors"){
    test_ctors();
}

//! Test priorities work in single-threaded configuration
//! \brief \ref requirement
TEST_CASE("continue_node priority support"){
    test_priority();
}

//! Test body copying and copy_body logic
//! \brief \ref interface
TEST_CASE("continue_node and body copying"){
    test_copies();
}

//! Test constructors
//! \brief \ref interface
TEST_CASE("continue_node constructors"){
    test_policy_ctors();
}

//! Test continue_node buffering
//! \brief \ref requirement
TEST_CASE("continue_node buffering"){
    test_buffering();
}

//! Test function_node broadcasting
//! \brief \ref requirement
TEST_CASE("continue_node broadcast"){
    test_forwarding();
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("continue_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
}

//! Test body execution
//! \brief \ref interface \ref requirement
TEST_CASE("continue body") {
    test_cont_body();
}
