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

// TODO: Add overlapping put / receive tests

#include "common/config.h"

#include "tbb/flow_graph.h"
#include "tbb/global_control.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/checktype.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"

#include <cstdio>


//! \file test_priority_queue_node.cpp
//! \brief Test for [flow_graph.priority_queue_node] specification


#define N 10
#define C 10

template< typename T >
void spin_try_get( tbb::flow::priority_queue_node<T> &q, T &value ) {
    while ( q.try_get(value) != true ) ;
}

template< typename T >
void check_item( T* next_value, T &value ) {
    int tid = value / N;
    int offset = value % N;
    CHECK_MESSAGE( next_value[tid] == T(offset), "" );
    ++next_value[tid];
}

template< typename T >
struct parallel_puts : utils::NoAssign {
    tbb::flow::priority_queue_node<T> &my_q;
    parallel_puts( tbb::flow::priority_queue_node<T> &q ) : my_q(q) {}
    void operator()(int i) const {
        for (int j = 0; j < N; ++j) {
            bool msg = my_q.try_put( T(N*i + j) );
            CHECK_MESSAGE( msg == true, "" );
        }
    }
};

template< typename T >
struct parallel_gets : utils::NoAssign {
    tbb::flow::priority_queue_node<T> &my_q;
    parallel_gets( tbb::flow::priority_queue_node<T> &q) : my_q(q) {}
    void operator()(int) const {
        T prev;
        spin_try_get( my_q, prev );
        for (int j = 0; j < N-1; ++j) {
            T v;
            spin_try_get( my_q, v );
            CHECK_MESSAGE(v < prev, "");
        }
    }
};

template< typename T >
struct parallel_put_get : utils::NoAssign {
    tbb::flow::priority_queue_node<T> &my_q;
    parallel_put_get( tbb::flow::priority_queue_node<T> &q ) : my_q(q) {}
    void operator()(int tid) const {
        for ( int i = 0; i < N; i+=C ) {
            int j_end = ( N < i + C ) ? N : i + C;
            // dump about C values into the Q
            for ( int j = i; j < j_end; ++j ) {
                CHECK_MESSAGE( my_q.try_put( T (N*tid + j ) ) == true, "" );
            }
            // receive about C values from the Q
            for ( int j = i; j < j_end; ++j ) {
                T v;
                spin_try_get( my_q, v );
            }
        }
    }
};

//
// Tests
//
// Item can be reserved, released, consumed ( single serial receiver )
//
template< typename T >
int test_reservation(int) {
    tbb::flow::graph g;

    // Simple tests
    tbb::flow::priority_queue_node<T> q(g);

    {

        T bogus_value(-1);

        q.try_put(T(1));
        q.try_put(T(2));
        q.try_put(T(3));
        g.wait_for_all();

        T v=bogus_value, w=bogus_value;
        CHECK_MESSAGE( q.try_reserve(v) == true, "" );
        CHECK_MESSAGE( v == T(3), "" );
        CHECK_MESSAGE( q.try_release() == true, "" );
        v = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_reserve(v) == true, "" );
        CHECK_MESSAGE( v == T(3), "" );
        CHECK_MESSAGE( q.try_consume() == true, "" );
        v = bogus_value;
        g.wait_for_all();

        CHECK_MESSAGE( q.try_get(v) == true, "" );
        CHECK_MESSAGE( v == T(2), "" );
        v = bogus_value;
        g.wait_for_all();

        CHECK_MESSAGE( q.try_reserve(v) == true, "" );
        CHECK_MESSAGE( v == T(1), "" );
        CHECK_MESSAGE( q.try_reserve(w) == false, "" );
        CHECK_MESSAGE( w == bogus_value, "" );
        CHECK_MESSAGE( q.try_get(w) == false, "" );
        CHECK_MESSAGE( w == bogus_value, "" );
        CHECK_MESSAGE( q.try_release() == true, "" );
        v = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_reserve(v) == true, "" );
        CHECK_MESSAGE( v == T(1), "" );
        CHECK_MESSAGE( q.try_consume() == true, "" );
        v = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_get(v) == false, "" );
    }
    return 0;
}

//
// Tests
//
// multiple parallel senders, items in FIFO (relatively to sender) order
// multiple parallel senders, multiple parallel receivers, items in FIFO order (relative to sender/receiver) and all items received
//   * overlapped puts / gets
//   * all puts finished before any getS
//
template< typename T >
int test_parallel(int num_threads) {
    tbb::flow::graph g;
    tbb::flow::priority_queue_node<T> q(g);
    tbb::flow::priority_queue_node<T> q2(g);
    tbb::flow::priority_queue_node<T> q3(g);
    T bogus_value(-1);
    T j = bogus_value;

    NativeParallelFor( num_threads, parallel_puts<T>(q) );
    for (int i = num_threads*N -1; i>=0; --i) {
        spin_try_get( q, j );
        CHECK_MESSAGE(j == i, "");
        j = bogus_value;
    }
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    NativeParallelFor( num_threads, parallel_puts<T>(q) );
    g.wait_for_all();
    NativeParallelFor( num_threads, parallel_gets<T>(q) );
    g.wait_for_all();
    j = bogus_value;
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    NativeParallelFor( num_threads, parallel_put_get<T>(q) );
    g.wait_for_all();
    j = bogus_value;
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::make_edge( q, q2 );
    tbb::flow::make_edge( q2, q3 );
    NativeParallelFor( num_threads, parallel_puts<T>(q) );
    g.wait_for_all();
    NativeParallelFor( num_threads, parallel_gets<T>(q3) );
    g.wait_for_all();
    j = bogus_value;
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( q2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( q3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    // test copy constructor
    CHECK_MESSAGE( remove_successor(q, q2) == true, "" );
    NativeParallelFor( num_threads, parallel_puts<T>(q) );
    tbb::flow::priority_queue_node<T> q_copy(q);
    g.wait_for_all();
    j = bogus_value;
    CHECK_MESSAGE( q_copy.try_get( j ) == false, "" );
    CHECK_MESSAGE( register_successor(q, q_copy) == true, "" );
    for (int i = num_threads*N -1; i>=0; --i) {
        spin_try_get( q_copy, j );
        CHECK_MESSAGE(j == i, "");
        j = bogus_value;
    }
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( q_copy.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    return 0;
}

//
// Tests
//
// Predecessors cannot be registered
// Empty Q rejects item requests
// Single serial sender, items in FIFO order
// Chained Qs ( 2 & 3 ), single sender, items at last Q in FIFO order
//

template< typename T >
int test_serial() {
    tbb::flow::graph g;
    T bogus_value(-1);

    tbb::flow::priority_queue_node<T> q(g);
    tbb::flow::priority_queue_node<T> q2(g);
    T j = bogus_value;

    //
    // Rejects attempts to add / remove predecessor
    // Rejects request from empty Q
    //
    CHECK_MESSAGE( register_predecessor(q, q2) == false, "" );
    CHECK_MESSAGE( remove_predecessor(q, q2) == false, "" );
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    //
    // Simple puts and gets
    //

    for (int i = 0; i < N; ++i)
        CHECK_MESSAGE( q.try_put( T(i) ), "" );
    for (int i = N-1; i >=0; --i) {
        j = bogus_value;
        spin_try_get( q, j );
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::make_edge( q, q2 );

    for (int i = 0; i < N; ++i)
        CHECK_MESSAGE( q.try_put( T(i) ), "" );
    g.wait_for_all();
    for (int i = N-1; i >= 0; --i) {
        j = bogus_value;
        spin_try_get( q2, j );
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::remove_edge( q, q2 );
    CHECK_MESSAGE( q.try_put( 1 ) == true, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == true, "" );
    CHECK_MESSAGE( j == 1, "" );

    tbb::flow::priority_queue_node<T> q3(g);
    tbb::flow::make_edge( q, q2 );
    tbb::flow::make_edge( q2, q3 );

    for (int i = 0; i < N; ++i)
        CHECK_MESSAGE(  q.try_put( T(i) ), "" );
    g.wait_for_all();
    for (int i = N-1; i >= 0; --i) {
        j = bogus_value;
        spin_try_get( q3, j );
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q2.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::remove_edge( q,  q2 );
    CHECK_MESSAGE( q.try_put( 1 ) == true, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( q.try_get( j ) == true, "" );
    CHECK_MESSAGE( j == 1, "" );

    return 0;
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    std::array<int, 3> messages_for_follows = { {0, 1, 2} };
    std::vector<int> messages_for_precedes = {0, 1, 2};

    follows_and_precedes_testing::test_follows <int, tbb::flow::priority_queue_node<int>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes <int, tbb::flow::priority_queue_node<int>>(messages_for_precedes);
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;
    broadcast_node<int> br(g);
    priority_queue_node<int> pq0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    using compare_type = std::greater<void>;
    priority_queue_node pq1(follows(br));
    static_assert(std::is_same_v<decltype(pq1), priority_queue_node<int>>);

    priority_queue_node pq2(follows(br), compare_type());
    static_assert(std::is_same_v<decltype(pq2), priority_queue_node<int, compare_type>>);

    priority_queue_node pq3(precedes(br));
    static_assert(std::is_same_v<decltype(pq3), priority_queue_node<int>>);

    priority_queue_node pq4(precedes(br), compare_type());
    static_assert(std::is_same_v<decltype(pq4), priority_queue_node<int, compare_type>>);
#endif

    priority_queue_node pq5(pq0);
    static_assert(std::is_same_v<decltype(pq5), priority_queue_node<int>>);
    g.wait_for_all();
}
#endif

//! Test serial, parallel behavior and reservation under parallelism
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Serial, parallel and reservation tests"){
    for (int p = 2; p <= 4; ++p) {
        tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, p);
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_serial<int>();
                test_reservation<int>(p);
                test_reservation<CheckType<int> >(p);
                test_parallel<int>(p);
            }
        );
	}
}

//! Test reset and cancellation
//! \brief \ref error_guessing
TEST_CASE("Reset tests"){
    INFO("Testing resets\n");
    test_resets<int,tbb::flow::priority_queue_node<int> >();
    test_resets<float,tbb::flow::priority_queue_node<float> >();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and precedes API"){
    test_follows_and_precedes_api();
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test decution guides
//! \brief \ref requirement
TEST_CASE("Test deduction guides"){
    test_deduction_guides();
}
#endif

