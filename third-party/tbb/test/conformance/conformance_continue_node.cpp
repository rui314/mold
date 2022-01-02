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

#define CONFORMANCE_CONTINUE_NODE

#include "conformance_flowgraph.h"

//! \file conformance_continue_node.cpp
//! \brief Test for [flow_graph.continue_node] specification

using output_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/true, /*copy_assign*/true/*enable for queue_node successor*/>;

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

template <typename ExpectedType, typename Body>
void test_deduction_guides_common(Body body) {
    using namespace tbb::flow;
    graph g;

    continue_node c1(g, body);
    static_assert(std::is_same_v<decltype(c1), continue_node<ExpectedType>>);

    continue_node c2(g, body, lightweight());
    static_assert(std::is_same_v<decltype(c2), continue_node<ExpectedType, lightweight>>);

    continue_node c3(g, 5, body);
    static_assert(std::is_same_v<decltype(c3), continue_node<ExpectedType>>);

    continue_node c4(g, 5, body, lightweight());
    static_assert(std::is_same_v<decltype(c4), continue_node<ExpectedType, lightweight>>);

    continue_node c5(g, body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(c5), continue_node<ExpectedType>>);

    continue_node c6(g, body, lightweight(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(c6), continue_node<ExpectedType, lightweight>>);

    continue_node c7(g, 5, body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(c7), continue_node<ExpectedType>>);

    continue_node c8(g, 5, body, lightweight(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(c8), continue_node<ExpectedType, lightweight>>);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    broadcast_node<continue_msg> b(g);

    continue_node c9(follows(b), body);
    static_assert(std::is_same_v<decltype(c9), continue_node<ExpectedType>>);

    continue_node c10(follows(b), body, lightweight());
    static_assert(std::is_same_v<decltype(c10), continue_node<ExpectedType, lightweight>>);

    continue_node c11(follows(b), 5, body);
    static_assert(std::is_same_v<decltype(c11), continue_node<ExpectedType>>);

    continue_node c12(follows(b), 5, body, lightweight());
    static_assert(std::is_same_v<decltype(c12), continue_node<ExpectedType, lightweight>>);

    continue_node c13(follows(b), body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(c13), continue_node<ExpectedType>>);

    continue_node c14(follows(b), body, lightweight(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(c14), continue_node<ExpectedType, lightweight>>);

    continue_node c15(follows(b), 5, body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(c15), continue_node<ExpectedType>>);

    continue_node c16(follows(b), 5, body, lightweight(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(c16), continue_node<ExpectedType, lightweight>>);
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

    continue_node c17(c1);
    static_assert(std::is_same_v<decltype(c17), continue_node<ExpectedType>>);
}

int continue_body_f(const tbb::flow::continue_msg&) { return 1; }
void continue_void_body_f(const tbb::flow::continue_msg&) {}

void test_deduction_guides() {
    using tbb::flow::continue_msg;
    test_deduction_guides_common<int>([](const continue_msg&)->int { return 1; } );
    test_deduction_guides_common<continue_msg>([](const continue_msg&) {});

    test_deduction_guides_common<int>([](const continue_msg&) mutable ->int { return 1; });
    test_deduction_guides_common<continue_msg>([](const continue_msg&) mutable {});

    test_deduction_guides_common<int>(continue_body_f);
    test_deduction_guides_common<continue_msg>(continue_void_body_f);
}

#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Test execution of node body
//! Test node can do try_put call
//! \brief \ref interface \ref requirement
TEST_CASE("continue body") {
    conformance::test_body_exec<oneapi::tbb::flow::continue_node<output_msg>, oneapi::tbb::flow::continue_msg, output_msg>();
}

//! Test continue_node is a graph_node, receiver<continue_msg>, and sender<Output>
//! \brief \ref interface
TEST_CASE("continue_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::continue_node<int>, oneapi::tbb::flow::continue_msg, int>();
    conformance::test_inheritance<oneapi::tbb::flow::continue_node<void*>, oneapi::tbb::flow::continue_msg, void*>();
    conformance::test_inheritance<oneapi::tbb::flow::continue_node<output_msg>, oneapi::tbb::flow::continue_msg, output_msg>();
}

//! Test body copying and copy_body logic
//! Test the body object passed to a node is copied
//! \brief \ref interface
TEST_CASE("continue_node and body copying"){
    conformance::test_copy_body_function<oneapi::tbb::flow::continue_node<int>, conformance::copy_counting_object<int, oneapi::tbb::flow::continue_msg>>();
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test node broadcast messages to successors
//! \brief \ref requirement
TEST_CASE("continue_node broadcast"){
    conformance::counting_functor<int> fun(conformance::expected);
    conformance::test_forwarding<oneapi::tbb::flow::continue_node<int>, oneapi::tbb::flow::continue_msg, int>(1, fun);
}

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("continue_node buffering"){
    conformance::dummy_functor<int> fun;
    conformance::test_buffering<oneapi::tbb::flow::continue_node<int>, oneapi::tbb::flow::continue_msg>(fun);
}

//! Test all node costructors
//! \brief \ref requirement
TEST_CASE("continue_node constructors"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::counting_functor<int> fun;

    continue_node<int> proto1(g, fun);
    continue_node<int> proto2(g, fun, oneapi::tbb::flow::node_priority_t(1));
    continue_node<int> proto3(g, 2, fun);
    continue_node<int> proto4(g, 2, fun, oneapi::tbb::flow::node_priority_t(1));

    continue_node<int, lightweight> lw_node1(g, fun, lightweight());
    continue_node<int, lightweight> lw_node2(g, fun, lightweight(), oneapi::tbb::flow::node_priority_t(1));
    continue_node<int, lightweight> lw_node3(g, 2, fun, lightweight());
    continue_node<int, lightweight> lw_node4(g, 2, fun, lightweight(), oneapi::tbb::flow::node_priority_t(1));
}

//! The node that is constructed has a reference to the same graph object as src,
//! has a copy of the initial body used by src, and only has a non-zero threshold if src is constructed with a non-zero threshold..
//! The predecessors and successors of src are not copied.
//! \brief \ref interface
TEST_CASE("continue_node copy constructor"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::dummy_functor<oneapi::tbb::flow::continue_msg> fun1;
    using counting_body = conformance::copy_counting_object<output_msg, oneapi::tbb::flow::continue_msg>;
    counting_body fun2;

    continue_node<oneapi::tbb::flow::continue_msg> node0(g, fun1);
    continue_node<output_msg> node1(g, 2, fun2);
    conformance::test_push_receiver<output_msg> node2(g);
    conformance::test_push_receiver<output_msg> node3(g);

    oneapi::tbb::flow::make_edge(node0, node1);
    oneapi::tbb::flow::make_edge(node1, node2);

    continue_node<output_msg> node_copy(node1);

    counting_body b2 = copy_body<counting_body, continue_node<output_msg>>(node_copy);

    CHECK_MESSAGE((fun2.copy_count + 1 < b2.copy_count), "constructor should copy bodies");

    oneapi::tbb::flow::make_edge(node_copy, node3);

    node_copy.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy successor, but copy number of predecessors");

    node_copy.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor, but copy number of predecessors");

    node1.try_put(oneapi::tbb::flow::continue_msg());
    node1.try_put(oneapi::tbb::flow::continue_msg());
    node0.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor, but copy number of predecessors");
}

//! Test continue_node wait for their predecessors to complete before executing, but no explicit data is passed across the incoming edges.
//! \brief \ref requirement
TEST_CASE("continue_node number_of_predecessors") {
    oneapi::tbb::flow::graph g;

    conformance::counting_functor<int> fun;

    oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg> node1(g, fun);
    oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg> node2(g, 1, fun);
    oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg> node3(g, 1, fun);
    oneapi::tbb::flow::continue_node<int> node4(g, fun); 

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node2, node4);
    oneapi::tbb::flow::make_edge(node1, node3);
    oneapi::tbb::flow::make_edge(node1, node3);
    oneapi::tbb::flow::remove_edge(node1, node3);
    oneapi::tbb::flow::make_edge(node3, node4);
    node3.try_put(oneapi::tbb::flow::continue_msg());
    node2.try_put(oneapi::tbb::flow::continue_msg());
    node1.try_put(oneapi::tbb::flow::continue_msg());

    g.wait_for_all();
    CHECK_MESSAGE((fun.execute_count == 4), "Node wait for their predecessors to complete before executing");
}

//! Test nodes for execution with priority in single-threaded configuration
//! \brief \ref requirement
TEST_CASE("continue_node priority support"){
    conformance::test_priority<oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg, int>, oneapi::tbb::flow::continue_msg>();
}

//! Test node Output class meet the CopyConstructible requirements.
//! \brief \ref requirement
TEST_CASE("continue_node Output class") {
    conformance::test_output_class<oneapi::tbb::flow::continue_node<conformance::copy_counting_object<int, oneapi::tbb::flow::continue_msg>>, conformance::copy_counting_object<int, oneapi::tbb::flow::continue_msg>>();
}

//! Test body `try_put' statement not wait for the execution of the body to complete
//! \brief \ref requirement
TEST_CASE("continue_node `try_put' statement not wait for the execution of the body to complete") {
    conformance::wait_flag_body body;
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::continue_node<oneapi::tbb::flow::continue_msg> node1(g, body);
    node1.try_put(oneapi::tbb::flow::continue_msg());
    conformance::wait_flag_body::flag.store(true);
    g.wait_for_all();
}
