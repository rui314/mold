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

#define CONFORMANCE_BUFFERING_NODES
#define CONFORMANCE_QUEUE_NODE

#include "conformance_flowgraph.h"

//! \file conformance_priority_queue_node.cpp
//! \brief Test for [flow_graph.priority_queue_node] specification

//! Test priority_queue_node single_push
//! \brief \ref requirement
TEST_CASE("priority_queue_node single_push"){
    conformance::test_forwarding_single_push<oneapi::tbb::flow::priority_queue_node<int>>();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("priority_queue_node buffering"){
    conformance::test_buffering<oneapi::tbb::flow::priority_queue_node<int>, int>();
}

//! Constructs an empty priority_queue_node that belongs to the same graph g as src.
//! Any intermediate state of src, including its links to predecessors and successors, is not copied.
//! \brief \ref requirement
TEST_CASE("priority_queue_node copy constructor"){
    conformance::test_copy_ctor_for_buffering_nodes<oneapi::tbb::flow::priority_queue_node<int>>();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("priority_queue_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::priority_queue_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::priority_queue_node<void*>, void*, void*>();
}

//! Test priority_queue_node node `try_put()` and `try_get()`
//! \brief \ref requirement
TEST_CASE("priority_queue_node methods"){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::priority_queue_node<int, std::greater<int>> testing_node(g);

    testing_node.try_put(2);
    testing_node.try_put(3);
    testing_node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((testing_node.try_get(tmp)), "Get should succeed");
    CHECK_MESSAGE((tmp == 1), "Values should get sorted");
    CHECK_MESSAGE((testing_node.try_get(tmp)), "Get should succeed");
    CHECK_MESSAGE((tmp == 2), "Values should get sorted");
    CHECK_MESSAGE((testing_node.try_get(tmp)), "Get should succeed");
    CHECK_MESSAGE((tmp == 3), "Values should get sorted");
}
