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

#include "conformance_flowgraph.h"

//! \file conformance_async_node.cpp
//! \brief Test for [flow_graph.async_node] specification

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true, /*copy_assign*/false>;
using output_msg = conformance::message</*default_ctor*/false, /*copy_ctor*/false, /*copy_assign*/false>;

//! Test async_node constructors
//! \brief \ref requirement
TEST_CASE("async_node constructors"){
    using namespace oneapi::tbb::flow;
    graph g;

    conformance::dummy_functor<int> fun;

    async_node<int, int> fn1(g, unlimited, fun);
    async_node<int, int> fn2(g, unlimited, fun, oneapi::tbb::flow::node_priority_t(1));

    async_node<int, int, lightweight> lw_node1(g, serial, fun, lightweight());
    async_node<int, int, lightweight> lw_node2(g, serial, fun, lightweight(), oneapi::tbb::flow::node_priority_t(1));
}

//! Test buffering property
//! \brief \ref requirement
TEST_CASE("async_node buffering") {
    conformance::dummy_functor<int> fun;
    conformance::test_buffering<oneapi::tbb::flow::async_node<input_msg, int>, input_msg>(oneapi::tbb::flow::unlimited, fun);
}

//! Test priorities work in single-threaded configuration
//! \brief \ref requirement
TEST_CASE("async_node priority support"){
    conformance::test_priority<oneapi::tbb::flow::async_node<input_msg, int>, input_msg>(oneapi::tbb::flow::unlimited);
}

//! The node that is constructed has a reference to the same graph object as src, has a copy of the initial body used by src, and has the same concurrency threshold as src.
//! The predecessors and successors of src are not copied.
//! \brief \ref requirement
TEST_CASE("async_node copy constructor"){
    conformance::test_copy_ctor<oneapi::tbb::flow::async_node<int, int>>();
}

//! Test calling async body
//! \brief \ref interface \ref requirement
TEST_CASE("Test async_node body") {
    conformance::test_body_exec<oneapi::tbb::flow::async_node<input_msg, output_msg>, input_msg, output_msg>(oneapi::tbb::flow::unlimited);
}

//! Test async_node inheritance relations
//! \brief \ref interface
TEST_CASE("async_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::async_node<int, int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::async_node<void*, float>, void*, float>();
    conformance::test_inheritance<oneapi::tbb::flow::async_node<input_msg, output_msg>, input_msg, output_msg>();
}

//! Test node broadcast messages to successors
//! \brief \ref requirement
TEST_CASE("async_node broadcast"){
    conformance::counting_functor<int> fun(conformance::expected);
    conformance::test_forwarding<oneapi::tbb::flow::async_node<input_msg, int>, input_msg, int>(1, oneapi::tbb::flow::unlimited, fun);
}

//! Test async_node has a user-settable concurrency limit. It can be set to one of predefined values. 
//! The user can also provide a value of type std::size_t to limit concurrency.
//! Test that not more than limited threads works in parallel.
//! \brief \ref requirement
TEST_CASE("concurrency follows set limits"){
    conformance::test_concurrency<oneapi::tbb::flow::async_node<int, int>>();
}

//! Test body copying and copy_body logic
//! Test the body object passed to a node is copied
//! \brief \ref interface
TEST_CASE("async_node body copying"){
    conformance::test_copy_body_function<oneapi::tbb::flow::async_node<int, int>, conformance::copy_counting_object<int>>(oneapi::tbb::flow::unlimited);
}

//! Test node reject the incoming message if the concurrency limit achieved.
//! \brief \ref interface
TEST_CASE("async_node with rejecting policy"){
    conformance::test_rejecting<oneapi::tbb::flow::async_node<int, int, oneapi::tbb::flow::rejecting>>();
}

//! Test node Input class meet the DefaultConstructible and CopyConstructible requirements and Output class meet the CopyConstructible requirements.
//! \brief \ref interface \ref requirement
TEST_CASE("Test async_node Output and Input class") {
    using Body = conformance::copy_counting_object<int>;
    conformance::test_output_input_class<oneapi::tbb::flow::async_node<Body, Body>, Body>();
}

//! Test the body of assync_node typically submits the messages to an external activity for processing outside of the graph.
//! \brief \ref interface
TEST_CASE("async_node with rejecting policy"){
    using async_node_type = tbb::flow::async_node<int, int>;
    using gateway_type = async_node_type::gateway_type;

    oneapi::tbb::flow::graph g;
    std::atomic<bool> flag{false};
    std::thread thr;
    async_node_type testing_node{
      g, tbb::flow::unlimited,
      [&](const int& input, gateway_type& gateway) {
          gateway.reserve_wait();
          thr = std::thread{[&]{
              flag = true;
              gateway.try_put(input);
              gateway.release_wait();
          }};
      }
    };

    testing_node.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE((flag.load()), "The body of assync_node must submits the messages to an external activity for processing outside of the graph");
    thr.join();
}
