/*
    Copyright (c) 2005-2021 Intel Corporation

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

#include "common/config.h"

#include "tbb/flow_graph.h"
#include "tbb/spin_rw_mutex.h"
#include "tbb/global_control.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"
#include "common/concepts_common.h"


//! \file test_function_node.cpp
//! \brief Test for [flow_graph.function_node] specification


#define N 100
#define MAX_NODES 4

//! Performs test on function nodes with limited concurrency and buffering
/** These tests check:
    1) that the number of executing copies never exceed the concurrency limit
    2) that the node never rejects
    3) that no items are lost
    and 4) all of this happens even if there are multiple predecessors and successors
*/

template<typename IO>
struct pass_through {
    IO operator()(const IO& i) { return i; }
};

template< typename InputType, typename OutputType, typename Body >
void buffered_levels( size_t concurrency, Body body ) {

   // Do for lc = 1 to concurrency level
   for ( size_t lc = 1; lc <= concurrency; ++lc ) {
   tbb::flow::graph g;

   // Set the execute_counter back to zero in the harness
   harness_graph_executor<InputType, OutputType>::execute_count = 0;
   // Set the number of current executors to zero.
   harness_graph_executor<InputType, OutputType>::current_executors = 0;
   // Set the max allowed executors to lc.  There is a check in the functor to make sure this is never exceeded.
   harness_graph_executor<InputType, OutputType>::max_executors = lc;

   // Create the function_node with the appropriate concurrency level, and use default buffering
   tbb::flow::function_node< InputType, OutputType > exe_node( g, lc, body );
   tbb::flow::function_node<InputType, InputType> pass_thru( g, tbb::flow::unlimited, pass_through<InputType>());

   // Create a vector of identical exe_nodes and pass_thrus
   std::vector< tbb::flow::function_node< InputType, OutputType > > exe_vec(2, exe_node);
   std::vector< tbb::flow::function_node< InputType, InputType > > pass_thru_vec(2, pass_thru);
   // Attach each pass_thru to its corresponding exe_node
   for (size_t node_idx=0; node_idx<exe_vec.size(); ++node_idx) {
       tbb::flow::make_edge(pass_thru_vec[node_idx], exe_vec[node_idx]);
   }

   // TODO: why the test is executed serially for the node pairs, not concurrently?
   for (size_t node_idx=0; node_idx<exe_vec.size(); ++node_idx) {
   // For num_receivers = 1 to MAX_NODES
       for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {
           // Create num_receivers counting receivers and connect the exe_vec[node_idx] to them.
           std::vector< std::shared_ptr<harness_mapped_receiver<OutputType>> > receivers;
           for (size_t i = 0; i < num_receivers; i++) {
               receivers.push_back( std::make_shared<harness_mapped_receiver<OutputType>>(g) );
           }

           for (size_t r = 0; r < num_receivers; ++r ) {
               tbb::flow::make_edge( exe_vec[node_idx], *receivers[r] );
           }

           // Do the test with varying numbers of senders
           std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders;
           for (size_t num_senders = 1; num_senders <= MAX_NODES; ++num_senders ) {
               // Create num_senders senders, set there message limit each to N, and connect them to
               // pass_thru_vec[node_idx]
               senders.clear();
               for (size_t s = 0; s < num_senders; ++s ) {
                   senders.push_back( std::make_shared<harness_counting_sender<InputType>>() );
                   senders.back()->my_limit = N;
                   senders.back()->register_successor(pass_thru_vec[node_idx] );
               }

               // Initialize the receivers so they know how many senders and messages to check for
               for (size_t r = 0; r < num_receivers; ++r ) {
                   receivers[r]->initialize_map( N, num_senders );
               }

               // Do the test
               utils::NativeParallelFor( (int)num_senders, parallel_put_until_limit<InputType>(senders) );
               g.wait_for_all();

               // confirm that each sender was requested from N times
               for (size_t s = 0; s < num_senders; ++s ) {
                   size_t n = senders[s]->my_received;
                   CHECK( n == N );
                   CHECK( senders[s]->my_receiver.load(std::memory_order_relaxed) == &pass_thru_vec[node_idx] );
               }
               // validate the receivers
               for (size_t r = 0; r < num_receivers; ++r ) {
                   receivers[r]->validate();
               }
           }
           for (size_t r = 0; r < num_receivers; ++r ) {
               tbb::flow::remove_edge( exe_vec[node_idx], *receivers[r] );
           }
           CHECK( exe_vec[node_idx].try_put( InputType() ) == true );
           g.wait_for_all();
           for (size_t r = 0; r < num_receivers; ++r ) {
               // since it's detached, nothing should have changed
               receivers[r]->validate();
           }

       } // for num_receivers
    } // for node_idx
    } // for concurrency level lc
}

const size_t Offset = 123;
std::atomic<size_t> global_execute_count;

struct inc_functor {

    std::atomic<size_t> local_execute_count;
    inc_functor( ) { local_execute_count = 0; }
    inc_functor( const inc_functor &f ) { local_execute_count = size_t(f.local_execute_count); }
    void operator=( const inc_functor &f ) { local_execute_count = size_t(f.local_execute_count); }

    int operator()( int i ) {
       ++global_execute_count;
       ++local_execute_count;
       return i;
    }

};

template< typename InputType, typename OutputType >
void buffered_levels_with_copy( size_t concurrency ) {

    // Do for lc = 1 to concurrency level
    for ( size_t lc = 1; lc <= concurrency; ++lc ) {
        tbb::flow::graph g;

        inc_functor cf;
        cf.local_execute_count = Offset;
        global_execute_count = Offset;

        tbb::flow::function_node< InputType, OutputType > exe_node( g, lc, cf );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {

            std::vector< std::shared_ptr<harness_mapped_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; i++) {
                receivers.push_back( std::make_shared<harness_mapped_receiver<OutputType>>(g) );
            }

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( exe_node, *receivers[r] );
            }

            std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders;
            for (size_t num_senders = 1; num_senders <= MAX_NODES; ++num_senders ) {
                senders.clear();
                for (size_t s = 0; s < num_senders; ++s ) {
                    senders.push_back( std::make_shared<harness_counting_sender<InputType>>() );
                    senders.back()->my_limit = N;
                    tbb::flow::make_edge( *senders.back(), exe_node );
                }

                for (size_t r = 0; r < num_receivers; ++r ) {
                    receivers[r]->initialize_map( N, num_senders );
                }

                utils::NativeParallelFor( (int)num_senders, parallel_put_until_limit<InputType>(senders) );
                g.wait_for_all();

                for (size_t s = 0; s < num_senders; ++s ) {
                    size_t n = senders[s]->my_received;
                    CHECK( n == N );
                    CHECK( senders[s]->my_receiver.load(std::memory_order_relaxed) == &exe_node );
                }
                for (size_t r = 0; r < num_receivers; ++r ) {
                    receivers[r]->validate();
                }
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( exe_node, *receivers[r] );
            }
            CHECK( exe_node.try_put( InputType() ) == true );
            g.wait_for_all();
            for (size_t r = 0; r < num_receivers; ++r ) {
                receivers[r]->validate();
            }
        }

        // validate that the local body matches the global execute_count and both are correct
        inc_functor body_copy = tbb::flow::copy_body<inc_functor>( exe_node );
        const size_t expected_count = N/2 * MAX_NODES * MAX_NODES * ( MAX_NODES + 1 ) + MAX_NODES + Offset;
        size_t global_count = global_execute_count;
        size_t inc_count = body_copy.local_execute_count;
        CHECK(global_count == expected_count);
        CHECK(global_count == inc_count );
        g.reset(tbb::flow::rf_reset_bodies);
        body_copy = tbb::flow::copy_body<inc_functor>( exe_node );
        inc_count = body_copy.local_execute_count;
        CHECK_MESSAGE( Offset == inc_count, "reset(rf_reset_bodies) did not reset functor" );
    }
}

template< typename InputType, typename OutputType >
void run_buffered_levels( int c ) {
    buffered_levels<InputType,OutputType>( c, []( InputType i ) -> OutputType { return harness_graph_executor<InputType, OutputType>::func(i); } );
    buffered_levels<InputType,OutputType>( c, &harness_graph_executor<InputType, OutputType>::func );
    buffered_levels<InputType,OutputType>( c, typename harness_graph_executor<InputType, OutputType>::functor() );
    buffered_levels_with_copy<InputType,OutputType>( c );
}


//! Performs test on executable nodes with limited concurrency
/** These tests check:
    1) that the nodes will accepts puts up to the concurrency limit,
    2) the nodes do not exceed the concurrency limit even when run with more threads (this is checked in the harness_graph_executor),
    3) the nodes will receive puts from multiple successors simultaneously,
    and 4) the nodes will send to multiple predecessors.
    There is no checking of the contents of the messages for corruption.
*/

template< typename InputType, typename OutputType, typename Body >
void concurrency_levels( size_t concurrency, Body body ) {

    for ( size_t lc = 1; lc <= concurrency; ++lc ) {
        tbb::flow::graph g;

        // Set the execute_counter back to zero in the harness
        harness_graph_executor<InputType, OutputType>::execute_count = 0;
        // Set the number of current executors to zero.
        harness_graph_executor<InputType, OutputType>::current_executors = 0;
        // Set the max allowed executors to lc. There is a check in the functor to make sure this is never exceeded.
        harness_graph_executor<InputType, OutputType>::max_executors = lc;

        typedef tbb::flow::function_node< InputType, OutputType, tbb::flow::rejecting > fnode_type;
        fnode_type exe_node( g, lc, body );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {

            std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; ++i) {
                receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
            }

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( exe_node, *receivers[r] );
            }

            std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders;

            for (size_t num_senders = 1; num_senders <= MAX_NODES; ++num_senders ) {
                senders.clear();
                {
                    // Exclusively lock m to prevent exe_node from finishing
                    tbb::spin_rw_mutex::scoped_lock l(
                        harness_graph_executor<InputType, OutputType>::template mutex_holder<tbb::spin_rw_mutex>::mutex
                    );

                    // put to lc level, it will accept and then block at m
                    for ( size_t c = 0 ; c < lc ; ++c ) {
                        CHECK( exe_node.try_put( InputType() ) == true );
                    }
                    // it only accepts to lc level
                    CHECK( exe_node.try_put( InputType() ) == false );

                    for (size_t s = 0; s < num_senders; ++s ) {
                        senders.push_back( std::make_shared<harness_counting_sender<InputType>>() );
                        // register a sender
                        senders.back()->my_limit = N;
                        exe_node.register_predecessor( *senders.back() );
                    }

                } // release lock at end of scope, setting the exe node free to continue
                // wait for graph to settle down
                g.wait_for_all();

                // confirm that each sender was requested from N times
                for (size_t s = 0; s < num_senders; ++s ) {
                    size_t n = senders[s]->my_received;
                    CHECK( n == N );
                    CHECK( senders[s]->my_receiver.load(std::memory_order_relaxed) == &exe_node );
                }
                // confirm that each receivers got N * num_senders + the initial lc puts
                for (size_t r = 0; r < num_receivers; ++r ) {
                    size_t n = receivers[r]->my_count;
                    CHECK( n == num_senders*N+lc );
                    receivers[r]->my_count = 0;
                }
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( exe_node, *receivers[r] );
            }
            CHECK( exe_node.try_put( InputType() ) == true );
            g.wait_for_all();
            for (size_t r = 0; r < num_receivers; ++r ) {
                CHECK( int(receivers[r]->my_count) == 0 );
            }
        }
    }
}


template< typename InputType, typename OutputType >
void run_concurrency_levels( int c ) {
    concurrency_levels<InputType,OutputType>( c, []( InputType i ) -> OutputType { return harness_graph_executor<InputType, OutputType>::template tfunc<tbb::spin_rw_mutex>(i); } );
    concurrency_levels<InputType,OutputType>( c, &harness_graph_executor<InputType, OutputType>::template tfunc<tbb::spin_rw_mutex> );
    concurrency_levels<InputType,OutputType>( c, typename harness_graph_executor<InputType, OutputType>::template tfunctor<tbb::spin_rw_mutex>() );
}


struct empty_no_assign {
   empty_no_assign() {}
   empty_no_assign( int ) {}
   operator int() { return 0; }
};

template< typename InputType >
struct parallel_puts : private utils::NoAssign {

    tbb::flow::receiver< InputType > * const my_exe_node;

    parallel_puts( tbb::flow::receiver< InputType > &exe_node ) : my_exe_node(&exe_node) {}

    void operator()( int ) const  {
        for ( int i = 0; i < N; ++i ) {
            // the nodes will accept all puts
            CHECK( my_exe_node->try_put( InputType() ) == true );
        }
    }

};

//! Performs test on executable nodes with unlimited concurrency
/** These tests check:
    1) that the nodes will accept all puts
    2) the nodes will receive puts from multiple predecessors simultaneously,
    and 3) the nodes will send to multiple successors.
    There is no checking of the contents of the messages for corruption.
*/

template< typename InputType, typename OutputType, typename Body >
void unlimited_concurrency( Body body ) {

    for (unsigned p = 1; p < 2*utils::MaxThread; ++p) {
        tbb::flow::graph g;
        tbb::flow::function_node< InputType, OutputType, tbb::flow::rejecting > exe_node( g, tbb::flow::unlimited, body );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {

            std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; ++i) {
                receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
            }

            harness_graph_executor<InputType, OutputType>::execute_count = 0;

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( exe_node, *receivers[r] );
            }

            utils::NativeParallelFor( p, parallel_puts<InputType>(exe_node) );
            g.wait_for_all();

            // 2) the nodes will receive puts from multiple predecessors simultaneously,
            size_t ec = harness_graph_executor<InputType, OutputType>::execute_count;
            CHECK( ec == p*N );
            for (size_t r = 0; r < num_receivers; ++r ) {
                size_t c = receivers[r]->my_count;
                // 3) the nodes will send to multiple successors.
                CHECK( c == p*N );
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( exe_node, *receivers[r] );
            }
            }
        }
    }

template< typename InputType, typename OutputType >
void run_unlimited_concurrency() {
    harness_graph_executor<InputType, OutputType>::max_executors = 0;
    unlimited_concurrency<InputType,OutputType>( []( InputType i ) -> OutputType { return harness_graph_executor<InputType, OutputType>::func(i); } );
    unlimited_concurrency<InputType,OutputType>( &harness_graph_executor<InputType, OutputType>::func );
    unlimited_concurrency<InputType,OutputType>( typename harness_graph_executor<InputType, OutputType>::functor() );
}

struct continue_msg_to_int {
    int my_int;
    continue_msg_to_int(int x) : my_int(x) {}
    int operator()(tbb::flow::continue_msg) { return my_int; }
};

void test_function_node_with_continue_msg_as_input() {
    // If this function terminates, then this test is successful
    tbb::flow::graph g;

    tbb::flow::broadcast_node<tbb::flow::continue_msg> Start(g);

    tbb::flow::function_node<tbb::flow::continue_msg, int, tbb::flow::rejecting> FN1( g, tbb::flow::serial, continue_msg_to_int(42));
    tbb::flow::function_node<tbb::flow::continue_msg, int, tbb::flow::rejecting> FN2( g, tbb::flow::serial, continue_msg_to_int(43));

    tbb::flow::make_edge( Start, FN1 );
    tbb::flow::make_edge( Start, FN2 );

    Start.try_put( tbb::flow::continue_msg() );
    g.wait_for_all();
}

//! Tests limited concurrency cases for nodes that accept data messages
void test_concurrency(int num_threads) {
    tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, num_threads);
    run_concurrency_levels<int,int>(num_threads);
    run_concurrency_levels<int,tbb::flow::continue_msg>(num_threads);
    run_buffered_levels<int, int>(num_threads);
    run_unlimited_concurrency<int,int>();
    run_unlimited_concurrency<int,empty_no_assign>();
    run_unlimited_concurrency<empty_no_assign,int>();
    run_unlimited_concurrency<empty_no_assign,empty_no_assign>();
    run_unlimited_concurrency<int,tbb::flow::continue_msg>();
    run_unlimited_concurrency<empty_no_assign,tbb::flow::continue_msg>();
    test_function_node_with_continue_msg_as_input();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = { msg_t() };

    pass_through<msg_t> pass_msg;

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::function_node<msg_t, msg_t>>
        (messages_for_follows, tbb::flow::unlimited, pass_msg);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::function_node<msg_t, msg_t>>
        (messages_for_precedes, tbb::flow::unlimited, pass_msg, tbb::flow::node_priority_t(1));
}
#endif


//! Test various node bodies with concurrency
//! \brief \ref error_guessing
TEST_CASE("Concurrency test") {
    for(unsigned int p = utils::MinThread; p <= utils::MaxThread; ++p ) {
        test_concurrency(p);
    }
}

//! NativeParallelFor testing with various concurrency settings
//! \brief \ref error_guessing
TEST_CASE("Lightweight testing"){
   lightweight_testing::test<tbb::flow::function_node>(10);
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Flowgraph node set test"){
     test_follows_and_precedes_api();
}
#endif

//! try_release and try_consume test
//! \brief \ref error_guessing
TEST_CASE("try_release try_consume"){
    tbb::flow::graph g;

    tbb::flow::function_node<int, int> fn(g, tbb::flow::unlimited, [](const int&v){return v;});

    CHECK_MESSAGE((fn.try_release()==false), "try_release should initially return false on a node");
    CHECK_MESSAGE((fn.try_consume()==false), "try_consume should initially return false on a node");
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for function_node input and output") {
    struct InputObject {
        InputObject() = default;
        InputObject( const InputObject& ) = default;
    };
    struct OutputObject : test_concepts::Copyable {};

    static_assert(utils::well_formed_instantiation<tbb::flow::function_node, InputObject, OutputObject>);
    static_assert(utils::well_formed_instantiation<tbb::flow::function_node, int, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::function_node, test_concepts::NonCopyable, OutputObject>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::function_node, test_concepts::NonDefaultInitializable, OutputObject>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::function_node, InputObject, test_concepts::NonCopyable>);
}

template <typename Input, typename Output, typename Body>
concept can_call_function_node_ctor = requires( tbb::flow::graph& graph, std::size_t concurrency, Body body,
                                                tbb::flow::node_priority_t priority, tbb::flow::buffer_node<int>& f ) {
    tbb::flow::function_node<Input, Output>(graph, concurrency, body);
    tbb::flow::function_node<Input, Output>(graph, concurrency, body, priority);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::function_node<Input, Output>(tbb::flow::follows(f), concurrency, body);
    tbb::flow::function_node<Input, Output>(tbb::flow::follows(f), concurrency, body, priority);
#endif
};

//! \brief \ref error_guessing
TEST_CASE("constraints for function_node body") {
    using input_type = int;
    using output_type = int;
    using namespace test_concepts::function_node_body;

    static_assert(can_call_function_node_ctor<input_type, output_type, Correct<input_type, output_type>>);
    static_assert(!can_call_function_node_ctor<input_type, output_type, NonCopyable<input_type, output_type>>);
    static_assert(!can_call_function_node_ctor<input_type, output_type, NonDestructible<input_type, output_type>>);
    static_assert(!can_call_function_node_ctor<input_type, output_type, NoOperatorRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_function_node_ctor<input_type, output_type, WrongInputRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_function_node_ctor<input_type, output_type, WrongReturnRoundBrackets<input_type, output_type>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
