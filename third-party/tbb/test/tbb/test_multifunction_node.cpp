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

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"
#include "common/concepts_common.h"


//! \file test_multifunction_node.cpp
//! \brief Test for [flow_graph.multifunction_node] specification


#if TBB_USE_DEBUG
#define N 16
#else
#define N 100
#endif
#define MAX_NODES 4

//! Performs test on function nodes with limited concurrency and buffering
/** These tests check:
    1) that the number of executing copies never exceed the concurrency limit
    2) that the node never rejects
    3) that no items are lost
    and 4) all of this happens even if there are multiple predecessors and successors
*/

//! exercise buffered multifunction_node.
template< typename InputType, typename OutputTuple, typename Body >
void buffered_levels( size_t concurrency, Body body ) {
    typedef typename std::tuple_element<0,OutputTuple>::type OutputType;
    // Do for lc = 1 to concurrency level
    for ( size_t lc = 1; lc <= concurrency; ++lc ) {
        tbb::flow::graph g;

        // Set the execute_counter back to zero in the harness
        harness_graph_multifunction_executor<InputType, OutputTuple>::execute_count = 0;
        // Set the number of current executors to zero.
        harness_graph_multifunction_executor<InputType, OutputTuple>::current_executors = 0;
        // Set the max allowed executors to lc.  There is a check in the functor to make sure this is never exceeded.
        harness_graph_multifunction_executor<InputType, OutputTuple>::max_executors = lc;

        // Create the function_node with the appropriate concurrency level, and use default buffering
        tbb::flow::multifunction_node< InputType, OutputTuple > exe_node( g, lc, body );

        //Create a vector of identical exe_nodes
        std::vector< tbb::flow::multifunction_node< InputType, OutputTuple > > exe_vec(2, exe_node);

        // exercise each of the copied nodes
        for (size_t node_idx=0; node_idx<exe_vec.size(); ++node_idx) {
            for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {
                // Create num_receivers counting receivers and connect the exe_vec[node_idx] to them.
                std::vector< std::shared_ptr<harness_mapped_receiver<OutputType>> > receivers;
                for (size_t i = 0; i < num_receivers; i++) {
                    receivers.push_back( std::make_shared<harness_mapped_receiver<OutputType>>(g) );
                }

                for (size_t r = 0; r < num_receivers; ++r ) {
                    tbb::flow::make_edge( tbb::flow::output_port<0>(exe_vec[node_idx]), *receivers[r] );
                }

                // Do the test with varying numbers of senders
                std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders;
                for (size_t num_senders = 1; num_senders <= MAX_NODES; ++num_senders ) {
                    // Create num_senders senders, set their message limit each to N, and connect
                    // them to the exe_vec[node_idx]
                    senders.clear();
                    for (size_t s = 0; s < num_senders; ++s ) {
                        senders.push_back( std::make_shared<harness_counting_sender<InputType>>() );
                        senders.back()->my_limit = N;
                        tbb::flow::make_edge( *senders.back(), exe_vec[node_idx] );
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
                        CHECK_MESSAGE( n == N, "" );
                        CHECK_MESSAGE( senders[s]->my_receiver.load(std::memory_order_relaxed) == &exe_vec[node_idx], "" );
                    }
                    // validate the receivers
                    for (size_t r = 0; r < num_receivers; ++r ) {
                        receivers[r]->validate();
                    }
                }
                for (size_t r = 0; r < num_receivers; ++r ) {
                    tbb::flow::remove_edge( tbb::flow::output_port<0>(exe_vec[node_idx]), *receivers[r] );
                }
                CHECK_MESSAGE( exe_vec[node_idx].try_put( InputType() ) == true, "" );
                g.wait_for_all();
                for (size_t r = 0; r < num_receivers; ++r ) {
                    // since it's detached, nothing should have changed
                    receivers[r]->validate();
                }
            }
        }
    }
}

const size_t Offset = 123;
std::atomic<size_t> global_execute_count;

struct inc_functor {

    std::atomic<size_t> local_execute_count;
    inc_functor( ) { local_execute_count = 0; }
    inc_functor( const inc_functor &f ) { local_execute_count = size_t(f.local_execute_count); }

    template<typename output_ports_type>
    void operator()( int i, output_ports_type &p ) {
       ++global_execute_count;
       ++local_execute_count;
       (void)std::get<0>(p).try_put(i);
    }

};

template< typename InputType, typename OutputTuple >
void buffered_levels_with_copy( size_t concurrency ) {
    typedef typename std::tuple_element<0,OutputTuple>::type OutputType;
    // Do for lc = 1 to concurrency level
    for ( size_t lc = 1; lc <= concurrency; ++lc ) {
        tbb::flow::graph g;

        inc_functor cf;
        cf.local_execute_count = Offset;
        global_execute_count = Offset;

        tbb::flow::multifunction_node< InputType, OutputTuple > exe_node( g, lc, cf );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {

            std::vector< std::shared_ptr<harness_mapped_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; i++) {
                receivers.push_back( std::make_shared<harness_mapped_receiver<OutputType>>(g) );
            }

            for (size_t r = 0; r < num_receivers; ++r ) {
               tbb::flow::make_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
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
                    CHECK_MESSAGE( n == N, "" );
                    CHECK_MESSAGE( senders[s]->my_receiver.load(std::memory_order_relaxed) == &exe_node, "" );
                }
                for (size_t r = 0; r < num_receivers; ++r ) {
                    receivers[r]->validate();
                }
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
            }
            CHECK_MESSAGE( exe_node.try_put( InputType() ) == true, "" );
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
        CHECK_MESSAGE( (global_count == expected_count && global_count == inc_count), "" );
    }
}

template< typename InputType, typename OutputTuple >
void run_buffered_levels( int c ) {
    typedef typename tbb::flow::multifunction_node<InputType,OutputTuple>::output_ports_type output_ports_type;
    buffered_levels<InputType,OutputTuple>( c, []( InputType i, output_ports_type &p ) { harness_graph_multifunction_executor<InputType, OutputTuple>::func(i,p); } );
    buffered_levels<InputType,OutputTuple>( c, &harness_graph_multifunction_executor<InputType, OutputTuple>::func );
    buffered_levels<InputType,OutputTuple>( c, typename harness_graph_multifunction_executor<InputType, OutputTuple>::functor() );
    buffered_levels_with_copy<InputType,OutputTuple>( c );
}


//! Performs test on executable nodes with limited concurrency
/** These tests check:
    1) that the nodes will accepts puts up to the concurrency limit,
    2) the nodes do not exceed the concurrency limit even when run with more threads (this is checked in the harness_graph_executor),
    3) the nodes will receive puts from multiple successors simultaneously,
    and 4) the nodes will send to multiple predecessors.
    There is no checking of the contents of the messages for corruption.
*/

template< typename InputType, typename OutputTuple, typename Body >
void concurrency_levels( size_t concurrency, Body body ) {
    typedef typename std::tuple_element<0,OutputTuple>::type OutputType;
    for ( size_t lc = 1; lc <= concurrency; ++lc ) {
        tbb::flow::graph g;

        // Set the execute_counter back to zero in the harness
        harness_graph_multifunction_executor<InputType, OutputTuple>::execute_count = 0;
        // Set the number of current executors to zero.
        harness_graph_multifunction_executor<InputType, OutputTuple>::current_executors = 0;
        // Set the max allowed executors to lc.  There is a check in the functor to make sure this is never exceeded.
        harness_graph_multifunction_executor<InputType, OutputTuple>::max_executors = lc;


        tbb::flow::multifunction_node< InputType, OutputTuple, tbb::flow::rejecting > exe_node( g, lc, body );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {

            std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; ++i) {
                receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
            }

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
            }

            std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders;

            for (size_t num_senders = 1; num_senders <= MAX_NODES; ++num_senders ) {
                {
                    // Exclusively lock m to prevent exe_node from finishing
                    tbb::spin_rw_mutex::scoped_lock l(
                        harness_graph_multifunction_executor< InputType, OutputTuple>::template mutex_holder<tbb::spin_rw_mutex>::mutex
                    );

                    // put to lc level, it will accept and then block at m
                    for ( size_t c = 0 ; c < lc ; ++c ) {
                        CHECK_MESSAGE( exe_node.try_put( InputType() ) == true, "" );
                    }
                    // it only accepts to lc level
                    CHECK_MESSAGE( exe_node.try_put( InputType() ) == false, "" );

                    senders.clear();
                    for (size_t s = 0; s < num_senders; ++s ) {
                        senders.push_back( std::make_shared<harness_counting_sender<InputType>>() );
                        senders.back()->my_limit = N;
                        exe_node.register_predecessor( *senders.back() );
                    }

                } // release lock at end of scope, setting the exe node free to continue
                // wait for graph to settle down
                g.wait_for_all();

                // confirm that each sender was requested from N times
                for (size_t s = 0; s < num_senders; ++s ) {
                    size_t n = senders[s]->my_received;
                    CHECK_MESSAGE( n == N, "" );
                    CHECK_MESSAGE( senders[s]->my_receiver.load(std::memory_order_relaxed) == &exe_node, "" );
                }
                // confirm that each receivers got N * num_senders + the initial lc puts
                for (size_t r = 0; r < num_receivers; ++r ) {
                    size_t n = receivers[r]->my_count;
                    CHECK_MESSAGE( n == num_senders*N+lc, "" );
                    receivers[r]->my_count = 0;
                }
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
            }
            CHECK_MESSAGE( exe_node.try_put( InputType() ) == true, "" );
            g.wait_for_all();
            for (size_t r = 0; r < num_receivers; ++r ) {
                CHECK_MESSAGE( int(receivers[r]->my_count) == 0, "" );
            }
        }
    }
}

template< typename InputType, typename OutputTuple >
void run_concurrency_levels( int c ) {
    typedef typename tbb::flow::multifunction_node<InputType,OutputTuple>::output_ports_type output_ports_type;
    concurrency_levels<InputType,OutputTuple>( c, []( InputType i, output_ports_type &p ) { harness_graph_multifunction_executor<InputType, OutputTuple>::template tfunc<tbb::spin_rw_mutex>(i,p); } );
    concurrency_levels<InputType,OutputTuple>( c, &harness_graph_multifunction_executor<InputType, OutputTuple>::template tfunc<tbb::spin_rw_mutex> );
    concurrency_levels<InputType,OutputTuple>( c, typename harness_graph_multifunction_executor<InputType, OutputTuple>::template tfunctor<tbb::spin_rw_mutex>() );
}


struct empty_no_assign {
   empty_no_assign() {}
   empty_no_assign( int ) {}
   operator int() { return 0; }
   operator int() const { return 0; }
};

template< typename InputType >
struct parallel_puts : private utils::NoAssign {

    tbb::flow::receiver< InputType > * const my_exe_node;

    parallel_puts( tbb::flow::receiver< InputType > &exe_node ) : my_exe_node(&exe_node) {}

    void operator()( int ) const  {
        for ( int i = 0; i < N; ++i ) {
            // the nodes will accept all puts
            CHECK_MESSAGE( my_exe_node->try_put( InputType() ) == true, "" );
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

template< typename InputType, typename OutputTuple, typename Body >
void unlimited_concurrency( Body body ) {
    typedef typename std::tuple_element<0,OutputTuple>::type OutputType;

    for (unsigned int p = 1; p < 2*utils::MaxThread; ++p) {
        tbb::flow::graph g;
        tbb::flow::multifunction_node< InputType, OutputTuple, tbb::flow::rejecting > exe_node( g, tbb::flow::unlimited, body );

        for (size_t num_receivers = 1; num_receivers <= MAX_NODES; ++num_receivers ) {
            std::vector< std::shared_ptr<harness_counting_receiver<OutputType>> > receivers;
            for (size_t i = 0; i < num_receivers; ++i) {
                receivers.push_back( std::make_shared<harness_counting_receiver<OutputType>>(g) );
            }

            harness_graph_multifunction_executor<InputType, OutputTuple>::execute_count = 0;

            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::make_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
            }

            utils::NativeParallelFor( p, parallel_puts<InputType>(exe_node) );
            g.wait_for_all();

            // 2) the nodes will receive puts from multiple predecessors simultaneously,
            size_t ec = harness_graph_multifunction_executor<InputType, OutputTuple>::execute_count;
            CHECK_MESSAGE( (unsigned int)ec == p*N, "" );
            for (size_t r = 0; r < num_receivers; ++r ) {
                size_t c = receivers[r]->my_count;
                // 3) the nodes will send to multiple successors.
                CHECK_MESSAGE( (unsigned int)c == p*N, "" );
            }
            for (size_t r = 0; r < num_receivers; ++r ) {
                tbb::flow::remove_edge( tbb::flow::output_port<0>(exe_node), *receivers[r] );
            }
        }
    }
}

template< typename InputType, typename OutputTuple >
void run_unlimited_concurrency() {
    harness_graph_multifunction_executor<InputType, OutputTuple>::max_executors = 0;
    typedef typename tbb::flow::multifunction_node<InputType,OutputTuple>::output_ports_type output_ports_type;
    unlimited_concurrency<InputType,OutputTuple>( []( InputType i, output_ports_type &p ) { harness_graph_multifunction_executor<InputType, OutputTuple>::func(i,p); } );
    unlimited_concurrency<InputType,OutputTuple>( &harness_graph_multifunction_executor<InputType, OutputTuple>::func );
    unlimited_concurrency<InputType,OutputTuple>( typename harness_graph_multifunction_executor<InputType, OutputTuple>::functor() );
}

template<typename InputType, typename OutputTuple>
struct oddEvenBody {
    typedef typename tbb::flow::multifunction_node<InputType,OutputTuple>::output_ports_type output_ports_type;
    typedef typename std::tuple_element<0,OutputTuple>::type EvenType;
    typedef typename std::tuple_element<1,OutputTuple>::type OddType;
    void operator() (const InputType &i, output_ports_type &p) {
        if((int)i % 2) {
            (void)std::get<1>(p).try_put(OddType(i));
        }
        else {
            (void)std::get<0>(p).try_put(EvenType(i));
        }
    }
};

template<typename InputType, typename OutputTuple >
void run_multiport_test(int num_threads) {
    typedef typename tbb::flow::multifunction_node<InputType, OutputTuple> mo_node_type;
    typedef typename std::tuple_element<0,OutputTuple>::type EvenType;
    typedef typename std::tuple_element<1,OutputTuple>::type OddType;
    tbb::task_arena arena(num_threads);
    arena.execute(
        [&] () {
            tbb::flow::graph g;
            mo_node_type mo_node(g, tbb::flow::unlimited, oddEvenBody<InputType, OutputTuple>() );

            tbb::flow::queue_node<EvenType> q0(g);
            tbb::flow::queue_node<OddType> q1(g);

            tbb::flow::make_edge(tbb::flow::output_port<0>(mo_node), q0);
            tbb::flow::make_edge(tbb::flow::output_port<1>(mo_node), q1);

            for(InputType i = 0; i < N; ++i) {
                mo_node.try_put(i);
            }

            g.wait_for_all();
            for(int i = 0; i < N/2; ++i) {
                EvenType e{};
                OddType o{};
                CHECK_MESSAGE( q0.try_get(e), "" );
                CHECK_MESSAGE( (int)e % 2 == 0, "" );
                CHECK_MESSAGE( q1.try_get(o), "" );
                CHECK_MESSAGE( (int)o % 2 == 1, "" );
            }
        }
    );
}

//! Tests limited concurrency cases for nodes that accept data messages
void test_concurrency(int num_threads) {
    tbb::task_arena arena(num_threads);
    arena.execute(
        [&] () {
            run_concurrency_levels<int,std::tuple<int> >(num_threads);
            run_concurrency_levels<int,std::tuple<tbb::flow::continue_msg> >(num_threads);
            run_buffered_levels<int, std::tuple<int> >(num_threads);
            run_unlimited_concurrency<int, std::tuple<int> >();
            run_unlimited_concurrency<int,std::tuple<empty_no_assign> >();
            run_unlimited_concurrency<empty_no_assign,std::tuple<int> >();
            run_unlimited_concurrency<empty_no_assign,std::tuple<empty_no_assign> >();
            run_unlimited_concurrency<int,std::tuple<tbb::flow::continue_msg> >();
            run_unlimited_concurrency<empty_no_assign,std::tuple<tbb::flow::continue_msg> >();
            run_multiport_test<int, std::tuple<int, int> >(num_threads);
            run_multiport_test<float, std::tuple<int, double> >(num_threads);
        }
    );
}

template<typename Policy>
void test_ports_return_references() {
    tbb::flow::graph g;
    typedef int InputType;
    typedef std::tuple<int> OutputTuple;
    tbb::flow::multifunction_node<InputType, OutputTuple, Policy> mf_node(
        g, tbb::flow::unlimited,
        &harness_graph_multifunction_executor<InputType, OutputTuple>::empty_func );
    test_output_ports_return_ref(mf_node);
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>

void test_precedes() {
    using namespace tbb::flow;

    using multinode = multifunction_node<int, std::tuple<int, int>>;

    graph g;

    buffer_node<int> b1(g);
    buffer_node<int> b2(g);

    multinode node(precedes(b1, b2), unlimited, [](const int& i, multinode::output_ports_type& op) -> void {
            if (i % 2)
                std::get<0>(op).try_put(i);
            else
                std::get<1>(op).try_put(i);
        }
    );

    node.try_put(0);
    node.try_put(1);
    g.wait_for_all();

    int storage;
    CHECK_MESSAGE((b1.try_get(storage) && !b1.try_get(storage) && b2.try_get(storage) && !b2.try_get(storage)),
            "Not exact edge quantity was made");
}

void test_follows_and_precedes_api() {
    using multinode = tbb::flow::multifunction_node<int, std::tuple<int, int, int>>;

    std::array<int, 3> messages_for_follows = { {0, 1, 2} };

    follows_and_precedes_testing::test_follows
        <int, tbb::flow::multifunction_node<int, std::tuple<int, int, int>>>
        (messages_for_follows, tbb::flow::unlimited, [](const int& i, multinode::output_ports_type& op) -> void {
            std::get<0>(op).try_put(i);
        });

    test_precedes();
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

//! Test various node bodies with concurrency
//! \brief \ref error_guessing
TEST_CASE("Concurrency test"){
    for( unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
       test_concurrency(p);
    }
}

//! Test return types of ports
//! \brief \ref error_guessing
TEST_CASE("Test ports retrurn references"){
    test_ports_return_references<tbb::flow::queueing>();
    test_ports_return_references<tbb::flow::rejecting>();
}

//! NativeParallelFor testing with various concurrency settings
//! \brief \ref error_guessing
TEST_CASE("Lightweight testing"){
    lightweight_testing::test<tbb::flow::multifunction_node>(10);
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows-precedes API"){
    test_follows_and_precedes_api();
}
//! Test priority constructor with follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test priority with follows and precedes"){
    using namespace tbb::flow;

    using multinode = multifunction_node<int, std::tuple<int, int>>;

    graph g;

    buffer_node<int> b1(g);
    buffer_node<int> b2(g);

    multinode node(precedes(b1, b2), unlimited, [](const int& i, multinode::output_ports_type& op) -> void {
            if (i % 2)
                std::get<0>(op).try_put(i);
            else
                std::get<1>(op).try_put(i);
        }
        , node_priority_t(0));

    node.try_put(0);
    node.try_put(1);
    g.wait_for_all();

    int storage;
    CHECK_MESSAGE((b1.try_get(storage) && !b1.try_get(storage) && b2.try_get(storage) && !b2.try_get(storage)),
            "Not exact edge quantity was made");
}

#endif

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for multifunction_node input") {
    struct InputObject {
        InputObject() = default;
        InputObject( const InputObject& ) = default;
    };

    static_assert(utils::well_formed_instantiation<tbb::flow::multifunction_node, InputObject, int>);
    static_assert(utils::well_formed_instantiation<tbb::flow::multifunction_node, int, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::multifunction_node, test_concepts::NonCopyable, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::multifunction_node, test_concepts::NonDefaultInitializable, int>);
}

template <typename Input, typename Output, typename Body>
concept can_call_multifunction_node_ctor = requires( tbb::flow::graph& graph, std::size_t concurrency, Body body,
                                                     tbb::flow::node_priority_t priority, tbb::flow::buffer_node<int>& f ) {
    tbb::flow::multifunction_node<Input, Output>(graph, concurrency, body);
    tbb::flow::multifunction_node<Input, Output>(graph, concurrency, body, priority);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::multifunction_node<Input, Output>(tbb::flow::follows(f), concurrency, body);
    tbb::flow::multifunction_node<Input, Output>(tbb::flow::follows(f), concurrency, body, priority);
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
};

//! \brief \ref error_guessing
TEST_CASE("constraints for multifunction_node body") {
    using input_type = int;
    using output_type = std::tuple<int>;
    using namespace test_concepts::multifunction_node_body;

    static_assert(can_call_multifunction_node_ctor<input_type, output_type, Correct<input_type, output_type>>);
    static_assert(!can_call_multifunction_node_ctor<input_type, output_type, NonCopyable<input_type, output_type>>);
    static_assert(!can_call_multifunction_node_ctor<input_type, output_type, NonDestructible<input_type, output_type>>);
    static_assert(!can_call_multifunction_node_ctor<input_type, output_type, NoOperatorRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_multifunction_node_ctor<input_type, output_type, WrongFirstInputOperatorRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_multifunction_node_ctor<input_type, output_type, WrongSecondInputOperatorRoundBrackets<input_type, output_type>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
