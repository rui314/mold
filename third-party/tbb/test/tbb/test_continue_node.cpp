/*
    Copyright (c) 2005-2023 Intel Corporation

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

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"
#include "common/concepts_common.h"


//! \file test_continue_node.cpp
//! \brief Test for [flow_graph.continue_node] specification


#define N 1000
#define MAX_NODES 4
#define C 8

// A class to use as a fake predecessor of continue_node
struct fake_continue_sender : public tbb::flow::sender<tbb::flow::continue_msg>
{
    typedef tbb::flow::sender<tbb::flow::continue_msg>::successor_type successor_type;
    // Define implementations of virtual methods that are abstract in the base class
    bool register_successor( successor_type& ) override { return false; }
    bool remove_successor( successor_type& )   override { return false; }
};

template< typename InputType >
struct parallel_puts {

    tbb::flow::receiver< InputType > * const my_exe_node;

    parallel_puts( tbb::flow::receiver< InputType > &exe_node ) : my_exe_node(&exe_node) {}
    parallel_puts& operator=(const parallel_puts&) = delete;

    void operator()( int ) const  {
        for ( int i = 0; i < N; ++i ) {
            // the nodes will accept all puts
            CHECK_MESSAGE( my_exe_node->try_put( InputType() ) == true, "" );
        }
    }

};

template< typename OutputType >
void run_continue_nodes( int p, tbb::flow::graph& g, tbb::flow::continue_node< OutputType >& n ) {
    fake_continue_sender fake_sender;
    for (size_t i = 0; i < N; ++i) {
        tbb::detail::d1::register_predecessor(n, fake_sender);
    }

    for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {
        std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
        for (size_t i = 0; i < num_receivers; ++i) {
            receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
        }
        harness_graph_executor<tbb::flow::continue_msg, OutputType>::execute_count = 0;

        for (size_t r = 0; r < num_receivers; ++r ) {
            tbb::flow::make_edge( n, *receivers[r] );
        }

        utils::NativeParallelFor( p, parallel_puts<tbb::flow::continue_msg>(n) );
        g.wait_for_all();

        // 2) the nodes will receive puts from multiple predecessors simultaneously,
        size_t ec = harness_graph_executor<tbb::flow::continue_msg, OutputType>::execute_count;
        CHECK_MESSAGE( (int)ec == p, "" );
        for (size_t r = 0; r < num_receivers; ++r ) {
            size_t c = receivers[r]->my_count;
            // 3) the nodes will send to multiple successors.
            CHECK_MESSAGE( (int)c == p, "" );
        }

        for (size_t r = 0; r < num_receivers; ++r ) {
            tbb::flow::remove_edge( n, *receivers[r] );
        }
    }
}

template< typename OutputType, typename Body >
void continue_nodes( Body body ) {
    for (int p = 1; p < 2*4/*MaxThread*/; ++p) {
        tbb::flow::graph g;
        tbb::flow::continue_node< OutputType > exe_node( g, body );
        run_continue_nodes( p, g, exe_node);
        exe_node.try_put(tbb::flow::continue_msg());
        tbb::flow::continue_node< OutputType > exe_node_copy( exe_node );
        run_continue_nodes( p, g, exe_node_copy);
    }
}

const size_t Offset = 123;
std::atomic<size_t> global_execute_count;

template< typename OutputType >
struct inc_functor {

    std::atomic<size_t> local_execute_count;
    inc_functor( ) { local_execute_count = 0; }
    inc_functor( const inc_functor &f ) { local_execute_count = size_t(f.local_execute_count); }
    void operator=(const inc_functor &f) { local_execute_count = size_t(f.local_execute_count); }

    OutputType operator()( tbb::flow::continue_msg ) {
       ++global_execute_count;
       ++local_execute_count;
       return OutputType();
    }

};

template< typename OutputType >
void continue_nodes_with_copy( ) {

    for (int p = 1; p < 2*4/*MaxThread*/; ++p) {
        tbb::flow::graph g;
        inc_functor<OutputType> cf;
        cf.local_execute_count = Offset;
        global_execute_count = Offset;

        tbb::flow::continue_node< OutputType > exe_node( g, cf );
        fake_continue_sender fake_sender;
        for (size_t i = 0; i < N; ++i) {
            tbb::detail::d1::register_predecessor(exe_node, fake_sender);
        }

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {
            std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; ++i) {
                receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
            }

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( exe_node, *receivers[r] );
            }

            utils::NativeParallelFor( p, parallel_puts<tbb::flow::continue_msg>(exe_node) );
            g.wait_for_all();

            // 2) the nodes will receive puts from multiple predecessors simultaneously,
            for (size_t r = 0; r < num_receivers; ++r ) {
                size_t c = receivers[r]->my_count;
                // 3) the nodes will send to multiple successors.
                CHECK_MESSAGE( (int)c == p, "" );
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( exe_node, *receivers[r] );
            }
        }

        // validate that the local body matches the global execute_count and both are correct
        inc_functor<OutputType> body_copy = tbb::flow::copy_body< inc_functor<OutputType> >( exe_node );
        const size_t expected_count = p*MAX_NODES + Offset;
        size_t global_count = global_execute_count;
        size_t inc_count = body_copy.local_execute_count;
        CHECK_MESSAGE( global_count == expected_count, "" );
        CHECK_MESSAGE( global_count == inc_count, "" );
        g.reset(tbb::flow::rf_reset_bodies);
        body_copy = tbb::flow::copy_body< inc_functor<OutputType> >( exe_node );
        inc_count = body_copy.local_execute_count;
        CHECK_MESSAGE( ( Offset == inc_count), "reset(rf_reset_bodies) did not reset functor" );

    }
}

template< typename OutputType >
void run_continue_nodes() {
    harness_graph_executor< tbb::flow::continue_msg, OutputType>::max_executors = 0;
    continue_nodes<OutputType>( []( tbb::flow::continue_msg i ) -> OutputType { return harness_graph_executor<tbb::flow::continue_msg, OutputType>::func(i); } );
    continue_nodes<OutputType>( &harness_graph_executor<tbb::flow::continue_msg, OutputType>::func );
    continue_nodes<OutputType>( typename harness_graph_executor<tbb::flow::continue_msg, OutputType>::functor() );
    continue_nodes_with_copy<OutputType>();
}

//! Tests limited concurrency cases for nodes that accept data messages
void test_concurrency(int num_threads) {
    tbb::task_arena arena(num_threads);
    arena.execute(
        [&] {
            run_continue_nodes<tbb::flow::continue_msg>();
            run_continue_nodes<int>();
            run_continue_nodes<utils::NoAssign>();
        }
    );
}
/*
 * Connection of two graphs is not currently supported, but works to some limited extent.
 * This test is included to check for backward compatibility. It checks that a continue_node
 * with predecessors in two different graphs receives the required
 * number of continue messages before it executes.
 */
using namespace tbb::flow;

struct add_to_counter {
    int* counter;
    add_to_counter(int& var):counter(&var){}
    void operator()(continue_msg){*counter+=1;}
};

void test_two_graphs(){
    int count=0;

    //graph g with broadcast_node and continue_node
    graph g;
    broadcast_node<continue_msg> start_g(g);
    continue_node<continue_msg> first_g(g, add_to_counter(count));

    //graph h with broadcast_node
    graph h;
    broadcast_node<continue_msg> start_h(h);

    //making two edges to first_g from the two graphs
    make_edge(start_g,first_g);
    make_edge(start_h, first_g);

    //two try_puts from the two graphs
    start_g.try_put(continue_msg());
    start_h.try_put(continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE( (count==1), "Not all continue messages received");

    //two try_puts from the graph that doesn't contain the node
    count=0;
    start_h.try_put(continue_msg());
    start_h.try_put(continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE( (count==1), "Not all continue messages received -1");

    //only one try_put
    count=0;
    start_g.try_put(continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE( (count==0), "Node executed without waiting for all predecessors");
}

struct lightweight_policy_body {
    const std::thread::id my_thread_id;
    std::atomic<size_t>& my_count;

    lightweight_policy_body( std::atomic<size_t>& count )
        : my_thread_id(std::this_thread::get_id()), my_count(count)
    {
        my_count = 0;
    }

    lightweight_policy_body( const lightweight_policy_body& ) = default;
    lightweight_policy_body& operator=( const lightweight_policy_body& ) = delete;

    void operator()( tbb::flow::continue_msg ) {
        ++my_count;
        std::thread::id body_thread_id = std::this_thread::get_id();
        CHECK_MESSAGE( (body_thread_id == my_thread_id), "Body executed as not lightweight");
    }
};

void test_lightweight_policy() {
    tbb::flow::graph g;
    std::atomic<size_t> count1;
    std::atomic<size_t> count2;
    tbb::flow::continue_node<tbb::flow::continue_msg, tbb::flow::lightweight>
        node1(g, lightweight_policy_body(count1));
    tbb::flow::continue_node<tbb::flow::continue_msg, tbb::flow::lightweight>
        node2(g, lightweight_policy_body(count2));

    tbb::flow::make_edge(node1, node2);
    const size_t n = 10;
    for(size_t i = 0; i < n; ++i) {
        node1.try_put(tbb::flow::continue_msg());
    }
    g.wait_for_all();

    lightweight_policy_body body1 = tbb::flow::copy_body<lightweight_policy_body>(node1);
    lightweight_policy_body body2 = tbb::flow::copy_body<lightweight_policy_body>(node2);
    CHECK_MESSAGE( (body1.my_count == n), "Body of the first node needs to be executed N times");
    CHECK_MESSAGE( (body2.my_count == n), "Body of the second node needs to be executed N times");
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows = { { msg_t(), msg_t(), msg_t() } };
    std::vector<msg_t> messages_for_precedes  = { msg_t() };

    auto pass_through = [](const msg_t& msg) { return msg; };

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::continue_node<msg_t>>
        (messages_for_follows, pass_through, node_priority_t(0));

    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::continue_node<msg_t>>
        (messages_for_precedes, /* number_of_predecessors = */0, pass_through, node_priority_t(1));
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

// TODO: use pass_through from test_function_node instead
template<typename T>
struct passing_body {
    T operator()(const T& val) {
        return val;
    }
};

/*
    The test covers the case when a node with non-default mutex type is a predecessor for continue_node,
    because there used to be a bug when make_edge(node, continue_node)
    did not update continue_node's predecesosor threshold
    since the specialization of node's successor_cache for a continue_node was not chosen.
*/
void test_successor_cache_specialization() {
    using namespace tbb::flow;

    graph g;

    broadcast_node<continue_msg> node_with_default_mutex_type(g);
    buffer_node<continue_msg> node_with_non_default_mutex_type(g);

    continue_node<continue_msg> node(g, passing_body<continue_msg>());

    make_edge(node_with_default_mutex_type, node);
    make_edge(node_with_non_default_mutex_type, node);

    buffer_node<continue_msg> buf(g);

    make_edge(node, buf);

    node_with_default_mutex_type.try_put(continue_msg());
    node_with_non_default_mutex_type.try_put(continue_msg());

    g.wait_for_all();

    continue_msg storage;
    CHECK_MESSAGE((buf.try_get(storage) && !buf.try_get(storage)),
                  "Wrong number of messages is passed via continue_node");
}

//! Test concurrent continue_node for correctness
//! \brief \ref error_guessing
TEST_CASE("Concurrency testing") {
    for( unsigned p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        test_concurrency(p);
    }
}

//! Test concurrent continue_node in separate graphs
//! \brief \ref error_guessing
TEST_CASE("Two graphs") { test_two_graphs(); }

//! Test basic behaviour with lightweight body
//! \brief \ref requirement \ref error_guessing
TEST_CASE( "Lightweight policy" ) { test_lightweight_policy(); }

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test deprecated follows and precedes API
//! \brief \ref error_guessing
TEST_CASE( "Support for follows and precedes API" ) { test_follows_and_precedes_api(); }
#endif

//! Test for successor cache specialization
//! \brief \ref regression
TEST_CASE( "Regression for successor cache specialization" ) {
    test_successor_cache_specialization();
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for continue_node input") {
    static_assert(utils::well_formed_instantiation<tbb::flow::continue_node, test_concepts::Copyable>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::continue_node, test_concepts::NonCopyable>);
}

template <typename Input, typename Body>
concept can_call_continue_node_ctor = requires( tbb::flow::graph& graph, Body body,
                                                tbb::flow::buffer_node<int>& f, std::size_t num,
                                                tbb::flow::node_priority_t priority  ) {
    tbb::flow::continue_node<Input>(graph, body);
    tbb::flow::continue_node<Input>(graph, body, priority);
    tbb::flow::continue_node<Input>(graph, num, body);
    tbb::flow::continue_node<Input>(graph, num, body, priority);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::continue_node<Input>(tbb::flow::follows(f), body);
    tbb::flow::continue_node<Input>(tbb::flow::follows(f), body, priority);
    tbb::flow::continue_node<Input>(tbb::flow::follows(f), num, body);
    tbb::flow::continue_node<Input>(tbb::flow::follows(f), num, body, priority);
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
};

//! \brief \ref error_guessing
TEST_CASE("constraints for continue_node body") {
    using output_type = int;
    using namespace test_concepts::continue_node_body;

    static_assert(can_call_continue_node_ctor<output_type, Correct<output_type>>);
    static_assert(!can_call_continue_node_ctor<output_type, NonCopyable<output_type>>);
    static_assert(!can_call_continue_node_ctor<output_type, NonDestructible<output_type>>);
    static_assert(!can_call_continue_node_ctor<output_type, NoOperatorRoundBrackets<output_type>>);
    static_assert(!can_call_continue_node_ctor<output_type, WrongInputOperatorRoundBrackets<output_type>>);
    static_assert(!can_call_continue_node_ctor<output_type, WrongReturnOperatorRoundBrackets<output_type>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
