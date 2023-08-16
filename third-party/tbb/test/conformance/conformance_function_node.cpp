/*
    Copyright (c) 2020-2023 Intel Corporation

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
#include "common/test_invoke.h"

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true, /*copy_assign*/false>;
using output_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/true, /*copy_assign*/false>;

//! \file conformance_function_node.cpp
//! \brief Test for [flow_graph.function_node] specification

/*
    Test node deduction guides
*/
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

int function_body_f(const int&) { return 1; }

template <typename Body>
void test_deduction_guides_common(Body body) {
    using namespace tbb::flow;
    graph g;

    function_node f1(g, unlimited, body);
    static_assert(std::is_same_v<decltype(f1), function_node<int, int>>);

    function_node f2(g, unlimited, body, rejecting());
    static_assert(std::is_same_v<decltype(f2), function_node<int, int, rejecting>>);

    function_node f3(g, unlimited, body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(f3), function_node<int, int>>);

    function_node f4(g, unlimited, body, rejecting(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(f4), function_node<int, int, rejecting>>);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    function_node f5(follows(f2), unlimited, body);
    static_assert(std::is_same_v<decltype(f5), function_node<int, int>>);

    function_node f6(follows(f5), unlimited, body, rejecting());
    static_assert(std::is_same_v<decltype(f6), function_node<int, int, rejecting>>);

    function_node f7(follows(f6), unlimited, body, node_priority_t(5));
    static_assert(std::is_same_v<decltype(f7), function_node<int, int>>);

    function_node f8(follows(f7), unlimited, body, rejecting(), node_priority_t(5));
    static_assert(std::is_same_v<decltype(f8), function_node<int, int, rejecting>>);
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

    function_node f9(f1);
    static_assert(std::is_same_v<decltype(f9), function_node<int, int>>);
}

void test_deduction_guides() {
    test_deduction_guides_common([](const int&)->int { return 1; });
    test_deduction_guides_common([](const int&) mutable ->int { return 1; });
    test_deduction_guides_common(function_body_f);
}

#endif

#if __TBB_CPP17_INVOKE_PRESENT

template <typename InputType, typename OutputType1, typename OutputType2,
          typename Body1, typename Body2>
void test_fn_invoke_basic(const Body1& body1, const Body2& body2) {
    using namespace oneapi::tbb::flow;

    graph g;

    function_node<InputType, OutputType1> f1(g, unlimited, body1);
    function_node<OutputType1, OutputType2> f2(g, unlimited, body2);
    buffer_node<OutputType2> buf(g);

    make_edge(f1, f2);
    make_edge(f2, buf);

    f1.try_put(InputType{OutputType1{1}});

    g.wait_for_all();

    std::size_t result = 0;
    CHECK(buf.try_get(result));
    CHECK(result == 1);
    CHECK(!buf.try_get(result));
}

void test_fn_invoke() {
    using output_type = test_invoke::SmartID<std::size_t>;
    using input_type = test_invoke::SmartID<output_type>;
    // Testing pointer to member function
    test_fn_invoke_basic<input_type, output_type, std::size_t>(&input_type::get_id, &output_type::get_id);
    // Testing pointer to member object
    test_fn_invoke_basic<input_type, output_type, std::size_t>(&input_type::id, &output_type::id);
}
#endif // __TBB_CPP17_INVOKE_PRESENT

//! Test calling function body
//! \brief \ref interface \ref requirement
TEST_CASE("Test function_node body") {
    conformance::test_body_exec<oneapi::tbb::flow::function_node<input_msg, output_msg>, input_msg, output_msg>(oneapi::tbb::flow::unlimited);
}

//! Test function_node constructors
//! \brief \ref requirement
TEST_CASE("function_node constructors"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::counting_functor<int> fun;

    function_node<int, int> fn1(g, unlimited, fun);
    function_node<int, int> fn2(g, unlimited, fun, oneapi::tbb::flow::node_priority_t(1));

    function_node<int, int, lightweight> lw_node1(g, serial, fun, lightweight());
    function_node<int, int, lightweight> lw_node2(g, serial, fun, lightweight(), oneapi::tbb::flow::node_priority_t(1));
}

//! The node that is constructed has a reference to the same graph object as src, has a copy of the initial body used by src, and has the same concurrency threshold as src.
//! The predecessors and successors of src are not copied.
//! \brief \ref requirement
TEST_CASE("function_node copy constructor"){
    conformance::test_copy_ctor<oneapi::tbb::flow::function_node<int, int>>();
}

//! Test node reject the incoming message if the concurrency limit achieved.
//! \brief \ref interface
TEST_CASE("function_node with rejecting policy"){
    conformance::test_rejecting<oneapi::tbb::flow::function_node<int, int, oneapi::tbb::flow::rejecting>>();
}

//! Test body copying and copy_body logic
//! Test the body object passed to a node is copied
//! \brief \ref interface
TEST_CASE("function_node and body copying"){
    conformance::test_copy_body_function<oneapi::tbb::flow::function_node<int, int>, conformance::copy_counting_object<int>>(oneapi::tbb::flow::unlimited);
}

//! Test function_node is a graph_node, receiver<Input>, and sender<Output>
//! \brief \ref interface
TEST_CASE("function_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::function_node<int, int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::function_node<void*, float>, void*, float>();
    conformance::test_inheritance<oneapi::tbb::flow::function_node<input_msg, output_msg>, input_msg, output_msg>();
}

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("function_node buffering"){
    conformance::dummy_functor<int> fun;
    conformance::test_buffering<oneapi::tbb::flow::function_node<input_msg, int, oneapi::tbb::flow::rejecting>, input_msg>(oneapi::tbb::flow::unlimited, fun);
    conformance::test_buffering<oneapi::tbb::flow::function_node<input_msg, int, oneapi::tbb::flow::queueing>, input_msg>(oneapi::tbb::flow::unlimited, fun);
}

//! Test node broadcast messages to successors
//! \brief \ref requirement
TEST_CASE("function_node broadcast"){
    conformance::counting_functor<int> fun(conformance::expected);
    conformance::test_forwarding<oneapi::tbb::flow::function_node<input_msg, int>, input_msg, int>(1, oneapi::tbb::flow::unlimited, fun);
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test priorities work in single-threaded configuration
//! \brief \ref requirement
TEST_CASE("function_node priority support"){
    conformance::test_priority<oneapi::tbb::flow::function_node<input_msg, int>, input_msg>(oneapi::tbb::flow::unlimited);
}

//! Test function_node has a user-settable concurrency limit. It can be set to one of predefined values.
//! The user can also provide a value of type std::size_t to limit concurrency.
//! Test that not more than limited threads works in parallel.
//! \brief \ref requirement
TEST_CASE("concurrency follows set limits"){
    conformance::test_concurrency<oneapi::tbb::flow::function_node<int, int>>();
}

//! Test node Input class meet the DefaultConstructible and CopyConstructible requirements and Output class meet the CopyConstructible requirements.
//! \brief \ref interface \ref requirement
TEST_CASE("Test function_node Output and Input class") {
    using Body = conformance::copy_counting_object<int>;
    conformance::test_output_input_class<oneapi::tbb::flow::function_node<Body, Body>, Body>();
}

#if __TBB_CPP17_INVOKE_PRESENT
//! Test that function_node uses std::invoke to execute the body
//! \brief \ref interface \ref requirement
TEST_CASE("Test function_node and std::invoke") {
    test_fn_invoke();
}
#endif
