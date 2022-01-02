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

//! \file conformance_queue_node.cpp
//! \brief Test for [flow_graph.queue_node] specification

//! Test queue_node single_push
//! \brief \ref requirement
TEST_CASE("queue_node single_push"){
    conformance::test_forwarding_single_push<oneapi::tbb::flow::queue_node<int>>();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("queue_node buffering"){
    conformance::test_buffering<oneapi::tbb::flow::queue_node<int>, int>();
}

//! Constructs an empty queue_node that belongs to the same graph g as src.
//! Any intermediate state of src, including its links to predecessors and successors, is not copied.
//! \brief \ref requirement
TEST_CASE("queue_node copy constructor"){
    conformance::test_copy_ctor_for_buffering_nodes<oneapi::tbb::flow::queue_node<int>>();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("queue_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::queue_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::queue_node<void*>, void*, void*>();
}

//! Test queue_node node `try_put()` and `try_get()`
//! \brief \ref requirement
TEST_CASE("queue_node methods"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::queue_node<int> testing_node(g);

    int tmp1 = -1;
    int tmp2 = -1;

    CHECK_MESSAGE((!testing_node.try_get(tmp1) && tmp1 == -1), "`try_get` must returns false if there is no non-reserved item currently in the node.");

    testing_node.try_put(1);
    testing_node.try_put(2);
    g.wait_for_all();

    testing_node.try_get(tmp1);
    CHECK_MESSAGE((tmp1 == 1), "Messages must be an FIFO order");

    testing_node.try_get(tmp2);
    CHECK_MESSAGE((tmp2 == 2), "Additional `try_get()' does not receive the same value as previous");
}
