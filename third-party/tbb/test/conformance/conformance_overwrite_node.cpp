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
#define CONFORMANCE_OVERWRITE_NODE

#include "conformance_flowgraph.h"

//! \file conformance_overwrite_node.cpp
//! \brief Test for [flow_graph.overwrite_node] specification

//! Test overwrite_node behavior
//! \brief \ref requirement
TEST_CASE("overwrite_node messages"){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::overwrite_node<int> testing_node(g);

    int tmp = -1;
    testing_node.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((testing_node.try_get(tmp) == true), "Descendant needs to receive a message");
    CHECK_MESSAGE((tmp == 1), "Descendant needs to receive a correct value");

    testing_node.try_put(2);
    g.wait_for_all();

    CHECK_MESSAGE((testing_node.try_get(tmp) == true), "Descendant needs to receive a message");
    CHECK_MESSAGE((tmp == 2), "Descendant needs to receive a correct value");
}

//! Test function_node broadcast
//! \brief \ref requirement
TEST_CASE("overwrite_node broadcast"){
    conformance::test_forwarding<oneapi::tbb::flow::overwrite_node<int>, int>(1);
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("overwrite_node buffering"){
    conformance::test_buffering<oneapi::tbb::flow::overwrite_node<int>, int>();
}

//! The node that is constructed has a reference to the same graph object as src,with an invalid internal buffer item. 
//! The buffered value and list of successors are not copied from src.
//! \brief \ref requirement
TEST_CASE("overwrite_node copy constructor"){
    conformance::test_copy_ctor_for_buffering_nodes<oneapi::tbb::flow::overwrite_node<int>>();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("overwrite_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::overwrite_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::overwrite_node<void*>, void*, void*>();
}

//! Test overwrite_node node constructor
//! \brief \ref requirement
TEST_CASE("overwrite_node constructor"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::overwrite_node<int> testing_node(g);

    int tmp = -1;
    CHECK_MESSAGE((!testing_node.is_valid()), "Constructed node has invalid internal buffer item");
    CHECK_MESSAGE((!testing_node.try_get(tmp)), "Gets from the node are non-destructive, but the first `try_get' fails");
}

//! Test overwrite_node node `is_valid()` and `clear()`
//! \brief \ref requirement
TEST_CASE("overwrite_node methods"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::overwrite_node<int> testing_node(g);

    CHECK_MESSAGE((!testing_node.is_valid()), "Constructed node has invalid internal buffer item");
    
    testing_node.try_put(1);
    
    CHECK_MESSAGE((testing_node.is_valid()), "Buffer must be valid after try_put call");

    testing_node.clear();

    CHECK_MESSAGE((!testing_node.is_valid()), "call `clear` invalidates the value held in the buffer.");
}

//! The following test shows the possibility to connect the node to a reserving join_node,
//! avoiding direct calls to the try_get() method from the body of the successor node
//! \brief \ref requirement
TEST_CASE("overwrite_node with reserving join_node as successor"){
    conformance::test_with_reserving_join_node_class<oneapi::tbb::flow::overwrite_node<int>>();
}
