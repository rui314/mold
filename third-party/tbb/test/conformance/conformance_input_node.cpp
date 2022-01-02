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

#define CONFORMANCE_INPUT_NODE

#include "conformance_flowgraph.h"

//! \file conformance_input_node.cpp
//! \brief Test for [flow_graph.input_node] specification

using output_msg = conformance::message<true, true, true>;

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
int input_body_f(tbb::flow_control&) { return 42; }

void test_deduction_guides() {
    using namespace tbb::flow;
    graph g;

    auto lambda = [](tbb::flow_control&) { return 42; };
    auto non_const_lambda = [](tbb::flow_control&) mutable { return 42; };

    // Tests for input_node(graph&, Body)
    input_node s1(g, lambda);
    static_assert(std::is_same_v<decltype(s1), input_node<int>>);

    input_node s2(g, non_const_lambda);
    static_assert(std::is_same_v<decltype(s2), input_node<int>>);

    input_node s3(g, input_body_f);
    static_assert(std::is_same_v<decltype(s3), input_node<int>>);

    input_node s4(s3);
    static_assert(std::is_same_v<decltype(s4), input_node<int>>);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    broadcast_node<int> bc(g);

    // Tests for input_node(const node_set<Args...>&, Body)
    input_node s5(precedes(bc), lambda);
    static_assert(std::is_same_v<decltype(s5), input_node<int>>);

    input_node s6(precedes(bc), non_const_lambda);
    static_assert(std::is_same_v<decltype(s6), input_node<int>>);

    input_node s7(precedes(bc), input_body_f);
    static_assert(std::is_same_v<decltype(s7), input_node<int>>);
#endif
    g.wait_for_all();
}

#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

template<typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE((std::is_base_of<graph_node, input_node<O>>::value), "input_node should be derived from graph_node");
    CHECK_MESSAGE((std::is_base_of<sender<O>, input_node<O>>::value), "input_node should be derived from sender<Output>");
    CHECK_MESSAGE((!std::is_base_of<receiver<O>, input_node<O>>::value), "input_node cannot have predecessors");
}

//! Test the body object passed to a node is copied
//! \brief \ref interface
TEST_CASE("input_node and body copying"){
    conformance::test_copy_body_function<oneapi::tbb::flow::input_node<int>, conformance::copy_counting_object<int>>();
}

//! The node that is constructed has a reference to the same graph object as src,
//! has a copy of the initial body used by src.
//! The successors of src are not copied.
//! \brief \ref requirement
TEST_CASE("input_node copy constructor"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::copy_counting_object<output_msg> fun2;

    input_node<output_msg> node1(g, fun2);
    conformance::test_push_receiver<output_msg> node2(g);
    conformance::test_push_receiver<output_msg> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);

    input_node<output_msg> node_copy(node1);

    conformance::copy_counting_object<output_msg> b2 = copy_body<conformance::copy_counting_object<output_msg>, input_node<output_msg>>(node_copy);

    CHECK_MESSAGE((fun2.copy_count + 1 < b2.copy_count), "constructor should copy bodies");

    oneapi::tbb::flow::make_edge(node_copy, node3);

    node_copy.activate();
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor");

    node1.activate();
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy successor");
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("input_node superclasses"){
    test_inheritance<int>();
    test_inheritance<void*>();
    test_inheritance<output_msg>();
}

//! Test input_node forwarding
//! \brief \ref requirement
TEST_CASE("input_node forwarding"){
    conformance::counting_functor<output_msg> fun(conformance::expected);
    conformance::test_forwarding<oneapi::tbb::flow::input_node<output_msg>, void, output_msg>(5, fun);
}

//! Test input_node buffering
//! \brief \ref requirement
TEST_CASE("input_node buffering"){
    conformance::dummy_functor<int> fun;
    conformance::test_buffering<oneapi::tbb::flow::input_node<int>, int>(fun);
}

//! Test calling input_node body
//! \brief \ref interface \ref requirement
TEST_CASE("input_node body") {
    oneapi::tbb::flow::graph g;
    constexpr std::size_t counting_threshold = 10;
    conformance::counting_functor<output_msg> fun(counting_threshold);

    oneapi::tbb::flow::input_node<output_msg> node1(g, fun);
    conformance::test_push_receiver<output_msg> node2(g);

    oneapi::tbb::flow::make_edge(node1, node2);

    node1.activate();
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == counting_threshold), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE((fun.execute_count == counting_threshold + 1), "Body of the node needs to be executed N + 1 times");
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test that measured concurrency respects set limits
//! \brief \ref requirement
TEST_CASE("concurrency follows set limits"){
    oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                  oneapi::tbb::this_task_arena::max_concurrency());


    utils::ConcurrencyTracker::Reset();
    oneapi::tbb::flow::graph g;
    conformance::concurrency_peak_checker_body counter(1);
    oneapi::tbb::flow::input_node<int> testing_node(g, counter);

    conformance::test_push_receiver<int> sink(g);

    make_edge(testing_node, sink);
    testing_node.activate();

    g.wait_for_all();
}

//! Test node Output class meet the CopyConstructible requirements.
//! \brief \ref interface \ref requirement
TEST_CASE("Test input_node Output class") {
    conformance::test_output_class<oneapi::tbb::flow::input_node<conformance::copy_counting_object<int>>>();
}

struct input_node_counter{
    static int count;
    int N;
    input_node_counter(int n) : N(n){};
    
    int operator()( oneapi::tbb::flow_control & fc ) {
       ++count;
       if(count > N){
           fc.stop();
           return N;
       }
       return N;
    }
};

struct function_node_counter{
    static int count;

    int operator()( int ) {
        ++count;
        utils::doDummyWork(1000000);
        CHECK_MESSAGE((input_node_counter::count <= function_node_counter::count + 1), "input_node `try_get()' call testing: a call to body is made only when the internal buffer is empty");
        return 1;
    }
};

int input_node_counter::count = 0;
int function_node_counter::count = 0;

//! Test input_node `try_get()' call testing: a call to body is made only when the internal buffer is empty.
//! \brief \ref requirement
TEST_CASE("input_node `try_get()' call testing: a call to body is made only when the internal buffer is empty.") {
    oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, 1);
    oneapi::tbb::flow::graph g;
    input_node_counter fun1(500);
    function_node_counter fun2;

    oneapi::tbb::flow::function_node <int, int, oneapi::tbb::flow::rejecting> fnode(g, oneapi::tbb::flow::serial, fun2);
    oneapi::tbb::flow::input_node<int> testing_node(g, fun1);

    make_edge(testing_node, fnode);
    testing_node.activate();

    g.wait_for_all();
}
