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

//! \file conformance_graph.cpp
//! \brief Test for [flow_graph.graph] specification

void test_continue_node_rf_reset_protocol(){
    using namespace oneapi::tbb::flow;
    graph g;

    std::atomic<bool> flag = {false};
    continue_node<int> source(g, 2, [&](const continue_msg&){ flag = true; return 1;});

    source.try_put(continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((flag == false), "Should be false");

    g.reset(rf_reset_protocol);

    source.try_put(continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((flag == false), "Internal number of predecessors reinitialized");

    source.try_put(continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((flag == true), "Should be true");
}

void test_input_node_rf_reset_protocol(){
    oneapi::tbb::flow::graph g;

    conformance::copy_counting_object<int> fun;

    oneapi::tbb::flow::input_node<int> node(g, fun);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);

    node.activate();
    g.wait_for_all();

    g.reset(oneapi::tbb::flow::rf_reset_protocol);

    int tmp = -1;
    CHECK_MESSAGE((node.try_get(tmp) == false), "Should be false");
}

template<typename Node>
void test_functional_nodes_rf_reset_protocol(){
    oneapi::tbb::flow::graph g;
    size_t concurrency_limit = 1;
    oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_limit);

    conformance::counting_functor<int> counting_body;
    Node f(g, oneapi::tbb::flow::serial, counting_body);

    f.try_put(0);
    f.try_put(0);
    CHECK_MESSAGE((counting_body.execute_count == 0), "Body should not be executed");
    g.reset(oneapi::tbb::flow::rf_reset_protocol);

    g.wait_for_all();
    CHECK_MESSAGE((counting_body.execute_count == 1), "Body should be executed");
}

template<typename Node, typename ...Args>
void test_buffering_nodes_rf_reset_protocol(Args... node_body){
    oneapi::tbb::flow::graph g;
    Node testing_node(g, node_body...);

    int tmp = -1;
    CHECK_MESSAGE((testing_node.try_get(tmp) == false), "try_get should not succeed");
    CHECK_MESSAGE((tmp == -1), "Value should not be updated");

    testing_node.try_put(1);
    g.wait_for_all();
    g.reset(oneapi::tbb::flow::rf_reset_protocol);

    tmp = -1;
    CHECK_MESSAGE((testing_node.try_get(tmp) == false), "try_get should not succeed");
    CHECK_MESSAGE((tmp == -1), "Value should not be updated");
    g.wait_for_all();
}

template<typename Node, typename InputType, typename ...Args>
void test_nodes_with_body_rf_reset_bodies(Args... node_args){
    oneapi::tbb::flow::graph g;
    conformance::counting_functor<int> counting_body(5);
    Node testing_node(g, node_args..., counting_body);

    testing_node.try_put(InputType());
    g.wait_for_all();

    CHECK_MESSAGE((counting_body.execute_count == 1), "Body should be executed");

    g.reset(oneapi::tbb::flow::rf_reset_bodies);
    testing_node.try_put(InputType());
    g.wait_for_all();

    CHECK_MESSAGE((counting_body.execute_count == 1), "Body should be replaced with a copy of the body");
}

void test_limiter_node_rf_reset_protocol(){
    oneapi::tbb::flow::graph g;

    constexpr int limit = 5;
    oneapi::tbb::flow::limiter_node<int> testing_node(g, limit);
    conformance::test_push_receiver<int> suc_node(g);

    oneapi::tbb::flow::make_edge(testing_node, suc_node);

    for(int i = 0; i < limit * 2; ++i)
        testing_node.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(suc_node).size() == limit), "Descendant of the node needs be receive limited number of messages");

    g.reset(oneapi::tbb::flow::rf_reset_protocol);

    for(int i = 0; i < limit * 2; ++i)
        testing_node.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(suc_node).size() == limit), "Descendant of the node needs be receive limited number of messages");
}

void test_join_node_rf_reset_protocol(){
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::join_node<std::tuple<int>, oneapi::tbb::flow::queueing> testing_node(g);

    oneapi::tbb::flow::input_port<0>(testing_node).try_put(1);
    
    g.wait_for_all();
    g.reset(oneapi::tbb::flow::rf_reset_protocol);

    std::tuple<int> tmp(0);
    CHECK_MESSAGE((!testing_node.try_get(tmp)), "All buffers must be emptied");
}

//! Graph reset
//! \brief \ref requirement
TEST_CASE("graph reset with rf_reset_protocol") {
    using namespace oneapi::tbb::flow;
    test_continue_node_rf_reset_protocol();
    test_input_node_rf_reset_protocol();
    test_functional_nodes_rf_reset_protocol<function_node<int, int, queueing>>();
    test_functional_nodes_rf_reset_protocol<multifunction_node<int, std::tuple<int>, queueing>>();
    test_functional_nodes_rf_reset_protocol<async_node<int, int, queueing>>();

    test_buffering_nodes_rf_reset_protocol<buffer_node<int>>();
    test_buffering_nodes_rf_reset_protocol<queue_node<int>>();
    test_buffering_nodes_rf_reset_protocol<overwrite_node<int>>();
    test_buffering_nodes_rf_reset_protocol<write_once_node<int>>();
    test_buffering_nodes_rf_reset_protocol<priority_queue_node<int>>();
    conformance::sequencer_functor<int> sequencer;
    test_buffering_nodes_rf_reset_protocol<sequencer_node<int>>(sequencer);

    test_limiter_node_rf_reset_protocol();
    test_join_node_rf_reset_protocol();
}

//! Graph reset rf_clear_edges
//! \brief \ref requirement
TEST_CASE("graph reset with rf_clear_edges") {
    oneapi::tbb::flow::graph g;
    using body = conformance::dummy_functor<int>;

    oneapi::tbb::flow::queue_node<int> successor(g);
    oneapi::tbb::flow::queue_node<std::tuple<int>> successor2(g);
    oneapi::tbb::flow::queue_node<oneapi::tbb::flow::indexer_node<int>::output_type> successor3(g);

    //node types
    oneapi::tbb::flow::continue_node<int> ct(g, body());
    oneapi::tbb::flow::split_node< std::tuple<int> > s(g);
    oneapi::tbb::flow::input_node<int> src(g, body());
    oneapi::tbb::flow::function_node<int, int> fxn(g, oneapi::tbb::flow::unlimited, body());
    oneapi::tbb::flow::multifunction_node<int, std::tuple<int, int> > m_fxn(g, oneapi::tbb::flow::unlimited, body());
    oneapi::tbb::flow::broadcast_node<int> bc(g);
    oneapi::tbb::flow::limiter_node<int> lim(g, 2);
    oneapi::tbb::flow::indexer_node<int> ind(g);
    oneapi::tbb::flow::join_node< std::tuple< int >, oneapi::tbb::flow::queueing > j(g);
    oneapi::tbb::flow::buffer_node<int> bf(g);
    oneapi::tbb::flow::priority_queue_node<int> pq(g);
    oneapi::tbb::flow::write_once_node<int> wo(g);
    oneapi::tbb::flow::overwrite_node<int> ovw(g);
    oneapi::tbb::flow::sequencer_node<int> seq(g, conformance::sequencer_functor<int>());

    oneapi::tbb::flow::make_edge(ct, successor);
    oneapi::tbb::flow::make_edge(s, successor);
    oneapi::tbb::flow::make_edge(src, successor);
    oneapi::tbb::flow::make_edge(fxn, successor);
    oneapi::tbb::flow::make_edge(m_fxn, successor);
    oneapi::tbb::flow::make_edge(bc, successor);
    oneapi::tbb::flow::make_edge(lim, successor);
    oneapi::tbb::flow::make_edge(ind, successor3);
    oneapi::tbb::flow::make_edge(j, successor2);
    oneapi::tbb::flow::make_edge(bf, successor);
    oneapi::tbb::flow::make_edge(pq, successor);
    oneapi::tbb::flow::make_edge(wo, successor);
    oneapi::tbb::flow::make_edge(ovw, successor);
    oneapi::tbb::flow::make_edge(seq, successor);

    g.wait_for_all();
    g.reset(oneapi::tbb::flow::rf_clear_edges);

    ct.try_put(oneapi::tbb::flow::continue_msg());
    s.try_put(std::tuple<int>{1});
    src.activate();
    fxn.try_put(1);
    m_fxn.try_put(1);
    bc.try_put(1);
    lim.try_put(1);
    oneapi::tbb::flow::input_port<0>(ind).try_put(1);
    oneapi::tbb::flow::input_port<0>(j).try_put(1);
    bf.try_put(1);
    pq.try_put(1);
    wo.try_put(1);
    ovw.try_put(1);
    seq.try_put(0);

    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(successor).size() == 0), "Message should not pass when edges doesn't exist");
    CHECK_MESSAGE((conformance::get_values(successor2).size() == 0), "Message should not pass when edge doesn't exist");
    CHECK_MESSAGE((conformance::get_values(successor3).size() == 0), "Message should not pass when edge doesn't exist");
}

//! Graph reset rf_reset_bodies
//! \brief \ref requirement
TEST_CASE("graph reset with rf_reset_bodies") {
    using namespace oneapi::tbb::flow;
    test_nodes_with_body_rf_reset_bodies<continue_node<int>, continue_msg>(serial);
    test_nodes_with_body_rf_reset_bodies<function_node<int, int>, int>(serial);
    test_nodes_with_body_rf_reset_bodies<multifunction_node<int, std::tuple<int>>, int>(serial);
    test_nodes_with_body_rf_reset_bodies<async_node<int, int>, int>(serial);

    graph g;
    conformance::counting_functor<int> counting_body(1);
    input_node<int> testing_node(g,counting_body);
    queue_node<int> q_node(g);

    make_edge(testing_node, q_node);

    testing_node.activate();
    g.wait_for_all();

    CHECK_MESSAGE((counting_body.execute_count == 2), "Body should be executed");

    g.reset(rf_reset_bodies);
    testing_node.activate();
    g.wait_for_all();

    CHECK_MESSAGE((counting_body.execute_count == 2), "Body should be replaced with a copy of the body");
}

//! Graph cancel
//! \brief \ref requirement
TEST_CASE("graph cancel") {
    oneapi::tbb::flow::graph g;
    CHECK_MESSAGE(!g.is_cancelled(), "Freshly created graph should not be cancelled." );

    g.cancel();
    CHECK_MESSAGE(!g.is_cancelled(), "Cancelled status should appear only after the wait_for_all() call." );

    g.wait_for_all();
    CHECK_MESSAGE(g.is_cancelled(), "Waiting should allow checking the cancellation status." );

    g.reset();
    CHECK_MESSAGE(!g.is_cancelled(), "Resetting must reset the cancellation status." );

    std::atomic<bool> cancelled(false);
    std::atomic<unsigned> executed(0);
    oneapi::tbb::flow::function_node<int> f(g, oneapi::tbb::flow::serial, [&](int) {
        ++executed;
        while( !cancelled.load(std::memory_order_relaxed) )
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
    });

    const unsigned N = 10;
    for( unsigned i = 0; i < N; ++i )
        f.try_put(0);

    std::thread thr([&] {
        while( !executed )
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        g.cancel();
        cancelled.store(true, std::memory_order_relaxed);
    });
    g.wait_for_all();
    thr.join();
    CHECK_MESSAGE(g.is_cancelled(), "Wait for all should not change the cancellation status." );
    CHECK_MESSAGE(1 == executed, "Buffered messages should be dropped by the cancelled graph." );
}
