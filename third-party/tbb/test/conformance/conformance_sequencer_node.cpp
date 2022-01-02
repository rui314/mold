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

#define SEQUENCER_NODE

#include "conformance_flowgraph.h"

//! \file conformance_sequencer_node.cpp
//! \brief Test for [flow_graph.sequencer_node] specification


#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <typename Body>
void test_deduction_guides_common(Body body) {
    using namespace tbb::flow;
    graph g;
    broadcast_node<int> br(g);

    sequencer_node s1(g, body);
    static_assert(std::is_same_v<decltype(s1), sequencer_node<int>>);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    sequencer_node s2(follows(br), body);
    static_assert(std::is_same_v<decltype(s2), sequencer_node<int>>);
#endif

    sequencer_node s3(s1);
    static_assert(std::is_same_v<decltype(s3), sequencer_node<int>>);
}

std::size_t sequencer_body_f(const int&) { return 1; }

void test_deduction_guides() {
    test_deduction_guides_common([](const int&)->std::size_t { return 1; });
    test_deduction_guides_common([](const int&) mutable ->std::size_t { return 1; });
    test_deduction_guides_common(sequencer_body_f);
}
#endif

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test sequencer_node single_push
//! \brief \ref requirement
TEST_CASE("sequencer_node single_push"){
    conformance::sequencer_functor<int> sequencer;
    conformance::test_forwarding_single_push<oneapi::tbb::flow::sequencer_node<int>>(sequencer);
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("sequencer_node buffering"){
    conformance::sequencer_functor<int> sequencer;
    conformance::test_buffering<oneapi::tbb::flow::sequencer_node<int>, int>(sequencer);
}

//! Constructs an empty sequencer_node that belongs to the same graph g as src.
//! Any intermediate state of src, including its links to predecessors and successors, is not copied.
//! \brief \ref requirement
TEST_CASE("sequencer_node copy constructor"){
    conformance::sequencer_functor<int> sequencer;
    conformance::test_copy_ctor_for_buffering_nodes<oneapi::tbb::flow::sequencer_node<int>>(sequencer);
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("sequencer_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::sequencer_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::sequencer_node<void*>, void*, void*>();
}

//! Test the sequencer_node rejects duplicate sequencer numbers
//! \brief \ref interface
TEST_CASE("sequencer_node rejects duplicate"){
    oneapi::tbb::flow::graph g;
    conformance::sequencer_functor<int> sequencer;

    oneapi::tbb::flow::sequencer_node<int> node(g, sequencer);

    node.try_put(1);

    CHECK_MESSAGE((node.try_put(1) == false), "sequencer_node must rejects duplicate sequencer numbers");
    g.wait_for_all();
}

//! Test queue_node node `try_put()` and `try_get()`
//! \brief \ref requirement
TEST_CASE("queue_node methods"){
    oneapi::tbb::flow::graph g;
    conformance::sequencer_functor<int> sequencer;

    oneapi::tbb::flow::sequencer_node<int> node(g, sequencer);

    node.try_put(1);
    node.try_put(0);
    node.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == true), "Getting from sequencer should succeed");
    CHECK_MESSAGE((tmp == 0), "Received value should be correct");

    tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == true), "Getting from sequencer should succeed");
    CHECK_MESSAGE((tmp == 1), "Received value should be correct");

    tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == false), "Getting from sequencer should not succeed");
}

//! The example demonstrates ordering capabilities of the sequencer_node. 
//! While being processed in parallel, the data is passed to the successor node in the exact same order it was read.
//! \brief \ref requirement
TEST_CASE("sequencer_node ordering"){
    using namespace oneapi::tbb::flow;
    using message = conformance::sequencer_functor<int>::seq_message;
    graph g;

    // Due to parallelism the node can push messages to its successors in any order
    function_node<message, message> process(g, unlimited, [] (message msg) {
        msg.data++;
        return msg;
    });

    sequencer_node<message> ordering(g, conformance::sequencer_functor<int>());

    std::atomic<std::size_t> counter{0};
    function_node<message> writer(g, tbb::flow::serial, [&] (const message& msg) {
        CHECK_MESSAGE((msg.id == counter++), "The data is passed to the successor node in the exact same order it was read");
    });

    tbb::flow::make_edge(process, ordering);
    tbb::flow::make_edge(ordering, writer);

    for (std::size_t i = 0; i < 100; ++i) {
        message msg = {i, 0};
        process.try_put(msg);
    }

    g.wait_for_all();
}
