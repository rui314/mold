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

//! \file conformance_limiter_node.cpp
//! \brief Test for [flow_graph.limiter_node] specification

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true/*enable for queue_node successor*/, /*copy_assign*/true/*enable for queue_node successor*/>;

//! Test limiter_node limiting
//! \brief \ref requirement
TEST_CASE("limiter_node limiting"){
    oneapi::tbb::flow::graph g;

    constexpr int limit = 5;
    oneapi::tbb::flow::limiter_node<input_msg> node1(g, limit);
    conformance::test_push_receiver<input_msg> node2(g);

    oneapi::tbb::flow::make_edge(node1, node2);

    for(int i = 0; i < limit * 2; ++i)
        node1.try_put(input_msg(1));
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == limit), "Descendant of the node needs be receive limited number of messages");
}

//! Test node broadcast messages to successors
//! \brief \ref requirement
TEST_CASE("limiter_node broadcast"){
    conformance::test_forwarding<oneapi::tbb::flow::limiter_node<int>, int>(1, 5);
    conformance::test_forwarding<oneapi::tbb::flow::limiter_node<input_msg>, input_msg>(1, 5);
}

//! Test node not buffered unsuccessful message, and try_get after rejection should not succeed.
//! \brief \ref requirement
TEST_CASE("limiter_node buffering"){
    conformance::test_buffering<oneapi::tbb::flow::limiter_node<int>, int>(5);
    conformance::test_buffering<oneapi::tbb::flow::limiter_node<int, int>, int>(5);
}

//! The node that is constructed has a reference to the same graph object as src, has the same threshold.
//! The predecessors and successors of src are not copied.
//! \brief \ref interface
TEST_CASE("limiter_node copy constructor"){
    using namespace oneapi::tbb::flow;
    graph g;

    limiter_node<int> node0(g, 1);
    limiter_node<int> node1(g, 1);
    conformance::test_push_receiver<int> node2(g);
    conformance::test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node0, node1);
    oneapi::tbb::flow::make_edge(node1, node2);

    limiter_node<int> node_copy(node1);

    oneapi::tbb::flow::make_edge(node_copy, node3);

    node_copy.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 1), "Copied node doesn`t copy successor");

    node_copy.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && conformance::get_values(node3).size() == 0), "Copied node copy threshold");

    node0.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor");
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("limiter_node superclasses"){
    conformance::test_inheritance<oneapi::tbb::flow::limiter_node<int>, int, int>();
    conformance::test_inheritance<oneapi::tbb::flow::limiter_node<float>, float, float>();
    conformance::test_inheritance<oneapi::tbb::flow::limiter_node<input_msg>, input_msg, input_msg>();
}

//! Test limiter_node decrementer
//! \brief \ref interface
TEST_CASE("limiter_node decrementer"){
    const int threshold = 5;
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::limiter_node<int, int> limit(g, threshold);
    oneapi::tbb::flow::queue_node<int> queue(g);
    make_edge(limit, queue);
    int m = 0;
    CHECK_MESSAGE(( limit.try_put( m++ )), "Newly constructed limiter node does not accept message." );
    CHECK_MESSAGE(limit.decrementer().try_put( -threshold ), // close limiter's gate
                   "Limiter node decrementer's port does not accept message." );
    CHECK_MESSAGE(( !limit.try_put( m++ )), "Closed limiter node's accepts message." );
    CHECK_MESSAGE(limit.decrementer().try_put( threshold + 5 ),  // open limiter's gate
                   "Limiter node decrementer's port does not accept message." );
    for( int i = 0; i < threshold; ++i )
        CHECK_MESSAGE(( limit.try_put( m++ )), "Limiter node does not accept message while open." );
    CHECK_MESSAGE(( !limit.try_put( m )), "Limiter node's gate is not closed." );
    g.wait_for_all();
    int expected[] = {0, 2, 3, 4, 5, 6};
    int actual = -1; m = 0;
    while( queue.try_get(actual) )
        CHECK_MESSAGE(actual == expected[m++], "" );
    CHECK_MESSAGE(( sizeof(expected) / sizeof(expected[0]) == m), "Not all messages have been processed." );
    g.wait_for_all();

    const size_t threshold2 = size_t(-1);
    oneapi::tbb::flow::limiter_node<int, long long> limit2(g, threshold2);
    make_edge(limit2, queue);
    CHECK_MESSAGE(( limit2.try_put( 1 )), "Newly constructed limiter node does not accept message." );
    long long decrement_value = (long long)( size_t(-1)/2 );
    CHECK_MESSAGE(limit2.decrementer().try_put( -decrement_value ),
                   "Limiter node decrementer's port does not accept message" );
    CHECK_MESSAGE(( limit2.try_put( 2 )), "Limiter's gate should not be closed yet." );
    CHECK_MESSAGE(limit2.decrementer().try_put( -decrement_value ),
                   "Limiter node decrementer's port does not accept message" );
    CHECK_MESSAGE(( !limit2.try_put( 3 )), "Overflow happened for internal counter." );
    int expected2[] = {1, 2};
    actual = -1; m = 0;
    while( queue.try_get(actual) )
        CHECK_MESSAGE(actual == expected2[m++], "" );
    CHECK_MESSAGE(( sizeof(expected2) / sizeof(expected2[0]) == m), "Not all messages have been processed." );
    g.wait_for_all();

    const size_t threshold3 = 10;
    oneapi::tbb::flow::limiter_node<int, long long> limit3(g, threshold3);
    make_edge(limit3, queue);
    long long decrement_value3 = 3;
    CHECK_MESSAGE(limit3.decrementer().try_put( -decrement_value3 ),
                   "Limiter node decrementer's port does not accept message" );

    m = 0;
    while( limit3.try_put( m ) ){ m++; };
    CHECK_MESSAGE(m == threshold3 - decrement_value3, "Not all messages have been accepted." );

    actual = -1; m = 0;
    while( queue.try_get(actual) ){
        CHECK_MESSAGE(actual == m++, "Not all messages have been processed." );
    }

    g.wait_for_all();
    CHECK_MESSAGE(m == threshold3 - decrement_value3, "Not all messages have been processed." );

    const size_t threshold4 = 10;
    oneapi::tbb::flow::limiter_node<int> limit4(g, threshold4);
    make_edge(limit4, queue);

    limit4.try_put(-1);
    CHECK_MESSAGE(limit4.decrementer().try_put(oneapi::tbb::flow::continue_msg()),
                   "Limiter node decrementer's port does not accept continue_msg" );

    m = 0;
    while( limit4.try_put( m ) ){ m++; };
    CHECK_MESSAGE(m == threshold4, "Not all messages have been accepted." );

    actual = -1; m = -1;
    while( queue.try_get(actual) ){
        CHECK_MESSAGE(actual == m++, "Not all messages have been processed." );
    }

    g.wait_for_all();
}
