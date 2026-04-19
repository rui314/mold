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

#define CONFORMANCE_MULTIFUNCTION_NODE

#include "conformance_flowgraph.h"
#include "common/test_invoke.h"

//! \file conformance_multifunction_node.cpp
//! \brief Test for [flow_graph.function_node] specification

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true, /*copy_assign*/false>;
using output_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/false, /*copy_assign*/false>;

/*
    Test function_node is a graph_node, receiver<Input>, and sender<Output>
*/
template<typename I, typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE((std::is_base_of<graph_node, multifunction_node<I, O>>::value), "multifunction_node should be derived from graph_node");
    CHECK_MESSAGE((std::is_base_of<receiver<I>, multifunction_node<I, O>>::value), "multifunction_node should be derived from receiver<Input>");
}

//! Test node reject the incoming message if the concurrency limit achieved.
//! \brief \ref interface
TEST_CASE("multifunction_node with rejecting policy"){
    conformance::test_rejecting<oneapi::tbb::flow::multifunction_node<int, std::tuple<int>, oneapi::tbb::flow::rejecting>>();
}

//! Test nodes for execution with priority in single-threaded configuration
//! \brief \ref interface
TEST_CASE("multifunction_node priority"){
    conformance::test_priority<oneapi::tbb::flow::multifunction_node<input_msg, std::tuple<int>>, input_msg>(oneapi::tbb::flow::unlimited);
}

//! Test function_node has a user-settable concurrency limit. It can be set to one of predefined values.
//! The user can also provide a value of type std::size_t to limit concurrency.
//! Test that not more than limited threads works in parallel.
//! \brief \ref interface
TEST_CASE("multifunction_node concurrency"){
    conformance::test_concurrency<oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>>();
}

//! Test all node constructors
//! \brief \ref interface
TEST_CASE("multifunction_node constructors"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::counting_functor<int> fun;

    multifunction_node<int, std::tuple<int>> fn1(g, unlimited, fun);
    multifunction_node<int, std::tuple<int>> fn2(g, unlimited, fun, oneapi::tbb::flow::node_priority_t(1));

    multifunction_node<int, std::tuple<int>, lightweight> lw_node1(g, serial, fun, lightweight());
    multifunction_node<int, std::tuple<int>, lightweight> lw_node2(g, serial, fun, lightweight(), oneapi::tbb::flow::node_priority_t(1));
}

//! The node that is constructed has a reference to the same graph object as src, has a copy of the initial body used by src, and has the same concurrency threshold as src.
//! The predecessors and successors of src are not copied.
//! \brief \ref interface
TEST_CASE("multifunction_node copy constructor"){
    conformance::test_copy_ctor<oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>>();
}

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("multifunction_node buffering"){
    conformance::dummy_functor<int> fun;
    conformance::test_buffering<oneapi::tbb::flow::multifunction_node<input_msg, std::tuple<int>,
    oneapi::tbb::flow::rejecting>, input_msg>(oneapi::tbb::flow::unlimited, fun);
}

//! Test multifunction_node broadcasting
//! \brief \ref requirement
TEST_CASE("multifunction_node broadcast"){
    conformance::counting_functor<int> fun(conformance::expected);
    conformance::test_forwarding<oneapi::tbb::flow::multifunction_node<input_msg, std::tuple<int>>, input_msg, int>(1, oneapi::tbb::flow::unlimited, fun);
}

//! Test the body object passed to a node is copied
//! \brief \ref interface
TEST_CASE("multifunction_node copy body"){
    conformance::test_copy_body_function<oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>, conformance::copy_counting_object<int>>(oneapi::tbb::flow::unlimited);
}

//! Test execution of node body
//! Test node can do try_put call
//! \brief \ref interface \ref requirement
TEST_CASE("multifunction_node body") {
    conformance::test_body_exec<oneapi::tbb::flow::multifunction_node<input_msg, std::tuple<output_msg>, oneapi::tbb::flow::rejecting>, input_msg, output_msg>(oneapi::tbb::flow::unlimited);
}

//! Test multifunction_node output_ports() returns a tuple of output ports.
//! \brief \ref interface \ref requirement
TEST_CASE("multifunction_node output_ports") {
    using namespace oneapi::tbb::flow;
    graph g;
    conformance::dummy_functor<int> fun;
    oneapi::tbb::flow::multifunction_node <int, std::tuple<int>> node(g, unlimited, fun);

    CHECK_MESSAGE((std::is_same<oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type&,
        decltype(node.output_ports())>::value), "multifunction_node output_ports should returns a tuple of output ports");
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("multifunction_node superclasses"){
    test_inheritance<int, std::tuple<int>>();
    test_inheritance<void*, std::tuple<float>>();
    test_inheritance<input_msg, std::tuple<output_msg>>();
}

//! Test node Input class meet the DefaultConstructible and CopyConstructible requirements and Output class meet the CopyConstructible requirements.
//! \brief \ref interface \ref requirement
TEST_CASE("Test function_node Output and Input class") {
    using Body = conformance::copy_counting_object<int>;
    conformance::test_output_input_class<oneapi::tbb::flow::multifunction_node<Body, std::tuple<Body>>, Body>();
}

#if __TBB_CPP17_INVOKE_PRESENT
//! Test that multifunction_node uses std::invoke to execute the body
//! \brief \ref interface \ref requirement
TEST_CASE("Test multifunction_node and std::invoke") {
    using namespace oneapi::tbb::flow;

    using output_type1 = test_invoke::SmartID<std::size_t>;
    using input_type = test_invoke::SmartID<output_type1>;

    using output_tuple1 = std::tuple<output_type1, output_type1>;
    using output_tuple2 = std::tuple<std::size_t>;

    using first_mf_node_type = multifunction_node<input_type, output_tuple1>;
    using second_mf_node_type = multifunction_node<output_type1, output_tuple2>;

    using first_ports_type = typename first_mf_node_type::output_ports_type;
    using second_ports_type = typename second_mf_node_type::output_ports_type;

    graph g;

    auto first_body = &input_type::template send_id<first_ports_type>;
    auto second_body = &output_type1::template send_id<second_ports_type>;

    first_mf_node_type mf1(g, unlimited, first_body);
    second_mf_node_type mf21(g, unlimited, second_body);
    second_mf_node_type mf22(g, unlimited, second_body);

    buffer_node<std::size_t> buf(g);

    make_edge(output_port<0>(mf1), mf21);
    make_edge(output_port<1>(mf1), mf22);

    make_edge(output_port<0>(mf21), buf);
    make_edge(output_port<0>(mf22), buf);

    mf1.try_put(input_type{output_type1{1}});

    g.wait_for_all();

    std::size_t buf_size = 0;
    std::size_t tmp = 0;
    while(buf.try_get(tmp)) {
        ++buf_size;
        CHECK(tmp == 1);
    }

    CHECK(buf_size == 2);
}
#endif
