/*
    Copyright (c) 2005-2024 Intel Corporation

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

#include "test_buffering_try_put_and_wait.h"

//! \file test_queue_node.cpp
//! \brief Test for [flow_graph.queue_node] specification


#define N 1000
#define C 10

template< typename T >
void spin_try_get( tbb::flow::queue_node<T> &q, T &value ) {
    int count = 0;
    while ( q.try_get(value) != true ) {
        if (count < 1000000) {
            ++count;
        }
        if (count == 1000000) {
            // Perhaps, we observe the missed wakeup. Enqueue a task to wake up threads.
            tbb::task_arena a(tbb::task_arena::attach{});
            a.enqueue([]{});
            ++count;
        }
    }
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

    tbb::flow::queue_node<T> &my_q;

    parallel_puts( tbb::flow::queue_node<T> &q ) : my_q(q) {}

    void operator()(int i) const {
        for (int j = 0; j < N; ++j) {
            bool msg = my_q.try_put( T(N*i + j) );
            CHECK_MESSAGE( msg == true, "" );
        }
    }

};

template< typename T >
struct touches {

    bool **my_touches;
    T **my_last_touch;
    int my_num_threads;

    touches( int num_threads ) : my_num_threads(num_threads) {
        my_last_touch = new T* [my_num_threads];
        my_touches = new bool* [my_num_threads];
        for ( int p = 0; p < my_num_threads; ++p) {
            my_last_touch[p] = new T[my_num_threads];
            for ( int p2 = 0; p2 < my_num_threads; ++p2)
                my_last_touch[p][p2] = -1;

            my_touches[p] = new bool[N*my_num_threads];
            for ( int n = 0; n < N*my_num_threads; ++n)
                my_touches[p][n] = false;
        }
    }

    ~touches() {
        for ( int p = 0; p < my_num_threads; ++p) {
            delete [] my_touches[p];
            delete [] my_last_touch[p];
        }
        delete [] my_touches;
        delete [] my_last_touch;
    }

    bool check( int tid, T v ) {
        int v_tid = v / N;
        if ( my_touches[tid][v] != false ) {
            printf("Error: value seen twice by local thread\n");
            return false;
        }
        if ( v <= my_last_touch[tid][v_tid] ) {
            printf("Error: value seen in wrong order by local thread\n");
            return false;
        }
        my_last_touch[tid][v_tid] = v;
        my_touches[tid][v] = true;
        return true;
    }

    bool validate_touches() {
        bool *all_touches = new bool[N*my_num_threads];
        for ( int n = 0; n < N*my_num_threads; ++n)
            all_touches[n] = false;

        for ( int p = 0; p < my_num_threads; ++p) {
            for ( int n = 0; n < N*my_num_threads; ++n) {
                if ( my_touches[p][n] == true ) {
                    CHECK_MESSAGE( ( all_touches[n] == false), "value see by more than one thread\n" );
                    all_touches[n] = true;
                }
            }
        }
        for ( int n = 0; n < N*my_num_threads; ++n) {
            if ( !all_touches[n] )
                printf("No touch at %d, my_num_threads = %d\n", n, my_num_threads);
            //CHECK_MESSAGE( ( all_touches[n] == true), "value not seen by any thread\n" );
        }
        delete [] all_touches;
        return true;
    }

};

template< typename T >
struct parallel_gets : utils::NoAssign {

    tbb::flow::queue_node<T> &my_q;
    touches<T> &my_touches;

    parallel_gets( tbb::flow::queue_node<T> &q, touches<T> &t) : my_q(q), my_touches(t) {}

    void operator()(int tid) const {
        for (int j = 0; j < N; ++j) {
            T v;
            spin_try_get( my_q, v );
            my_touches.check( tid, v );
        }
    }

};

template< typename T >
struct parallel_put_get : utils::NoAssign {

    tbb::flow::queue_node<T> &my_q;
    touches<T> &my_touches;

    parallel_put_get( tbb::flow::queue_node<T> &q, touches<T> &t ) : my_q(q), my_touches(t) {}

    void operator()(int tid) const {

        for ( int i = 0; i < N; i+=C ) {
            int j_end = ( N < i + C ) ? N : i + C;
            // dump about C values into the Q
            for ( int j = i; j < j_end; ++j ) {
                CHECK_MESSAGE( my_q.try_put( T (N*tid + j ) ) == true, "" );
            }
            // receiver about C values from the Q
            for ( int j = i; j < j_end; ++j ) {
                T v;
                spin_try_get( my_q, v );
                my_touches.check( tid, v );
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
int test_reservation() {
    tbb::flow::graph g;
    T bogus_value(-1);

    // Simple tests
    tbb::flow::queue_node<T> q(g);

    q.try_put(T(1));
    q.try_put(T(2));
    q.try_put(T(3));

    T v;
    CHECK_MESSAGE( q.reserve_item(v) == true, "" );
    CHECK_MESSAGE( v == T(1), "" );
    CHECK_MESSAGE( q.release_reservation() == true, "" );
    v = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( q.reserve_item(v) == true, "" );
    CHECK_MESSAGE( v == T(1), "" );
    CHECK_MESSAGE( q.consume_reservation() == true, "" );
    v = bogus_value;
    g.wait_for_all();

    CHECK_MESSAGE( q.try_get(v) == true, "" );
    CHECK_MESSAGE( v == T(2), "" );
    v = bogus_value;
    g.wait_for_all();

    CHECK_MESSAGE( q.reserve_item(v) == true, "" );
    CHECK_MESSAGE( v == T(3), "" );
    CHECK_MESSAGE( q.release_reservation() == true, "" );
    v = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( q.reserve_item(v) == true, "" );
    CHECK_MESSAGE( v == T(3), "" );
    CHECK_MESSAGE( q.consume_reservation() == true, "" );
    v = bogus_value;
    g.wait_for_all();

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
    tbb::flow::queue_node<T> q(g);
    tbb::flow::queue_node<T> q2(g);
    tbb::flow::queue_node<T> q3(g);
    {
        Checker< T > my_check;
        T bogus_value(-1);
        T j = bogus_value;
        utils::NativeParallelFor( num_threads, parallel_puts<T>(q) );

        T *next_value = new T[num_threads];
        for (int tid = 0; tid < num_threads; ++tid) next_value[tid] = T(0);

        for (int i = 0; i < num_threads * N; ++i ) {
            spin_try_get( q, j );
            check_item( next_value, j );
            j = bogus_value;
        }
        for (int tid = 0; tid < num_threads; ++tid)  {
            CHECK_MESSAGE( next_value[tid] == T(N), "" );
        }
        delete[] next_value;

        j = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        utils::NativeParallelFor( num_threads, parallel_puts<T>(q) );

        {
            touches< T > t( num_threads );
            utils::NativeParallelFor( num_threads, parallel_gets<T>(q, t) );
            g.wait_for_all();
            CHECK_MESSAGE( t.validate_touches(), "" );
        }
        j = bogus_value;
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        g.wait_for_all();
        {
            touches< T > t2( num_threads );
            utils::NativeParallelFor( num_threads, parallel_put_get<T>(q, t2) );
            g.wait_for_all();
            CHECK_MESSAGE( t2.validate_touches(), "" );
        }
        j = bogus_value;
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        tbb::flow::make_edge( q, q2 );
        tbb::flow::make_edge( q2, q3 );

        utils::NativeParallelFor( num_threads, parallel_puts<T>(q) );
        {
            touches< T > t3( num_threads );
            utils::NativeParallelFor( num_threads, parallel_gets<T>(q3, t3) );
            g.wait_for_all();
            CHECK_MESSAGE( t3.validate_touches(), "" );
        }
        j = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        g.wait_for_all();
        CHECK_MESSAGE( q2.try_get( j ) == false, "" );
        g.wait_for_all();
        CHECK_MESSAGE( q3.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        // test copy constructor
        CHECK_MESSAGE( remove_successor( q, q2 ), "" );
        utils::NativeParallelFor( num_threads, parallel_puts<T>(q) );
        tbb::flow::queue_node<T> q_copy(q);
        j = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q_copy.try_get( j ) == false, "" );
        CHECK_MESSAGE( register_successor( q, q_copy ) == true, "" );
        {
            touches< T > t( num_threads );
            utils::NativeParallelFor( num_threads, parallel_gets<T>(q_copy, t) );
            g.wait_for_all();
            CHECK_MESSAGE( t.validate_touches(), "" );
        }
        j = bogus_value;
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );
        CHECK_MESSAGE( q_copy.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );
    }

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
    tbb::flow::queue_node<T> q(g);
    tbb::flow::queue_node<T> q2(g);
    {   // destroy the graph after manipulating it, and see if all the items in the buffers
        // have been destroyed before the graph
        Checker<T> my_check;  // if CheckType< U > count constructions and destructions
        T bogus_value(-1);
        T j = bogus_value;

        //
        // Rejects attempts to add / remove predecessor
        // Rejects request from empty Q
        //
        CHECK_MESSAGE( register_predecessor( q, q2 ) == false, "" );
        CHECK_MESSAGE( remove_predecessor( q, q2 ) == false, "" );
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        //
        // Simple puts and gets
        //

        for (int i = 0; i < N; ++i) {
            bool msg = q.try_put( T(i) );
            CHECK_MESSAGE( msg == true, "" );
        }


        for (int i = 0; i < N; ++i) {
            j = bogus_value;
            spin_try_get( q, j );
            CHECK_MESSAGE( i == j, "" );
        }
        j = bogus_value;
        g.wait_for_all();
        CHECK_MESSAGE( q.try_get( j ) == false, "" );
        CHECK_MESSAGE( j == bogus_value, "" );

        tbb::flow::make_edge( q, q2 );

        for (int i = 0; i < N; ++i) {
            bool msg = q.try_put( T(i) );
            CHECK_MESSAGE( msg == true, "" );
        }


        for (int i = 0; i < N; ++i) {
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

        tbb::flow::queue_node<T> q3(g);
        tbb::flow::make_edge( q, q2 );
        tbb::flow::make_edge( q2, q3 );

        for (int i = 0; i < N; ++i) {
            bool msg = q.try_put( T(i) );
            CHECK_MESSAGE( msg == true, "" );
        }

        for (int i = 0; i < N; ++i) {
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
    }

    return 0;
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    std::array<int, 3> messages_for_follows = { {0, 1, 2} };
    std::vector<int> messages_for_precedes = {0, 1, 2};

    follows_and_precedes_testing::test_follows <int, tbb::flow::queue_node<int>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes <int, tbb::flow::queue_node<int>>(messages_for_precedes);
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;
    graph g;
    broadcast_node<int> br(g);
    queue_node<int> q0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    queue_node q1(follows(br));
    static_assert(std::is_same_v<decltype(q1), queue_node<int>>);

    queue_node q2(precedes(br));
    static_assert(std::is_same_v<decltype(q2), queue_node<int>>);
#endif

    queue_node q3(q0);
    static_assert(std::is_same_v<decltype(q3), queue_node<int>>);
    g.wait_for_all();
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
void test_queue_node_try_put_and_wait() {
    using namespace test_try_put_and_wait;

    std::vector<int> start_work_items;
    std::vector<int> new_work_items;
    int wait_message = 10;

    for (int i = 0; i < wait_message; ++i) {
        start_work_items.emplace_back(i);
        new_work_items.emplace_back(i + 1 + wait_message);
    }

    // Test push
    // test_buffer_push tests the graph
    // buffer1 -> function -> buffer2 -> writer
    //     function is a queueing serial function_node that submits new_work_items once wait_message arrives
    //     writer is an unlimited function_node that writes an item into the processed_items vector
    // Test steps
    //     1. push start_work_items into the buffer1
    //     2. buffer1.try_put_and_wait(wait_message);
    //     3. g.wait_for_all()
    // test_buffer_push returns the index from which the items processed during wait_for_all() starts
    {
        std::vector<int> processed_items;

        std::size_t after_start = test_buffer_push<tbb::flow::queue_node<int>>(start_work_items, wait_message,
                                                                               new_work_items, processed_items);

        // Expected effect:
        // During buffer1.try_put_and_wait()
        //     1. start_work_items would be pushed to buffer1
        //     2. wait_message would be pushed to buffer1
        //     3. forward_task on buffer1 would transfer all of the items to the function_node in FIFO order
        //     4. the first item would occupy concurrency of function, other items would be pushed to the queue
        //     5. function would process start_work_items and push them to the buffer2
        //     6. wait_message would be processed last and add new_work_items to buffer1
        //     7. forward_task on buffer2 would transfer start_work_items in FIFO order and the wait_message to the writer
        //     8.  try_put_and_wait exits since wait_message is completed
        // During g.wait_for_all()
        //     10. forward_task for new_work_items in buffer1 would be spawned and put items in function in FIFO order
        //     11. function_node would process and push forward items from the queue in FIFO order
        // Expected items processing - { start_work_items FIFO, wait_message, new_work_items FIFO }

        std::size_t check_index = 0;
        CHECK_MESSAGE(after_start == start_work_items.size() + 1,
                      "try_put_and_wait should process start_work_items and the wait_message");
        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item,
                          "try_put_and_wait should process start_work_items FIFO");
        }

        CHECK_MESSAGE(processed_items[check_index++] == wait_message,
                      "try_put_and_wait should process wait_message after start_work_items");

        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item,
                          "wait_for_all should process new_work_items FIFO");
        }
        CHECK(check_index == processed_items.size());
    } // Test push

    // Test pull
    // test_buffer_pull tests the graph
    // buffer -> function
    //     function is a rejecting serial function_node that submits new_work_items once wait_message arrives
    //     and writes the processed item into the processed_items
    // Test steps
    //     1. push the occupier message to the function
    //     2. push start_work_items into the buffer
    //     3. buffer.try_put_and_wait(wait_message)
    //     4. g.wait_for_all()
    // test_buffer_pull returns the index from which the items processed during wait_for_all() starts

    {
        std::vector<int> processed_items;
        int occupier = 42;

        std::size_t after_start = test_buffer_pull<tbb::flow::queue_node<int>>(start_work_items, wait_message, occupier,
                                                                               new_work_items, processed_items);

        // Expected effect
        // 0. task for occupier processing would be spawned by the function
        // During buffer.try_put_and_wait()
        //     1. start_work_items would be pushed to the buffer
        //     2. wait_message would be pushed to the buffer
        //     3. forward_task would try to push items to the function, but would fail
        //        and set the edge to the pull state
        //     4. occupier would be processed
        //     5. items would be taken from the buffer by function in FIFO order
        //     6. wait_message would be taken last and push new_work_items to the buffer
        // During wait_for_all()
        //     7. new_work_items would be taken from the buffer in FIFO order
        // Expected items processing { occupier, start_work_items FIFO, wait_message, new_work_items FIFO }

        std::size_t check_index = 0;

        CHECK_MESSAGE(after_start == start_work_items.size() + 2,
                      "start_work_items, occupier and wait_message should be processed by try_put_and_wait");
        CHECK_MESSAGE(processed_items[check_index++] == occupier, "Unexpected items processing by try_put_and_wait");
        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item,
                          "try_put_and_wait should process start_work_items FIFO");
        }
        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected items processing by try_put_and_wait");

        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item,
                          "try_put_and_wait should process new_work_items FIFO");
        }
        CHECK(check_index == processed_items.size());
    }

    // Test reserve
    {
        int thresholds[] = { 1, 2 };

        for (int threshold : thresholds) {
            std::vector<int> processed_items;

            // test_buffer_reserve tests the following graph
            // buffer -> limiter -> function
            //  function is a rejecting serial function_node that puts an item to the decrementer port
            //  of the limiter inside of the body

            std::size_t after_start = test_buffer_reserve<tbb::flow::queue_node<int>>(threshold,
                start_work_items, wait_message, new_work_items, processed_items);

            // Expected effect:
            // 1. start_work_items would be pushed to the buffer
            // 2. wait_message_would be pushed to the buffer
            // 3. forward task of the buffer would push the first message to the limiter node.
            //    Since the limiter threshold is not reached, it would be directly passed to the function
            // 4. function would spawn the task for the first message processing
            // 5. the first would be processed
            // 6. decrementer.try_put() would be called and the limiter node would
            //    process all of the items from the buffer using the try_reserve/try_consume/try_release semantics
            // 7. When the wait_message would be taken from the queue, the try_put_and_wait would exit

            std::size_t check_index = 0;

            CHECK_MESSAGE(after_start == start_work_items.size() + 1,
                          "try_put_and_wait should start_work_items and wait_message");
            for (auto item : start_work_items) {
                CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
            }
            CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected wait_message processing");

            for (auto item : new_work_items) {
                CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
            }
        }
    }
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT

//! Test serial, parallel behavior and reservation under parallelism
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Parallel, serial test"){
    for (int p = 2; p <= 4; ++p) {
        tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, p);
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_serial<int>();
                test_serial<CheckType<int> >();
                test_parallel<int>(p);
                test_parallel<CheckType<int> >(p);
            }
        );
	}
}

//! Test reset and cancellation
//! \brief \ref error_guessing
TEST_CASE("Resets test"){
    INFO("Testing resets\n");
    test_resets<int, tbb::flow::queue_node<int> >();
    test_resets<float, tbb::flow::queue_node<float> >();
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
TEST_CASE("Deduction guides"){
    test_deduction_guides();
}
#endif

//! Test operations on a reserved queue_node
//! \brief \ref error_guessing
TEST_CASE("queue_node with reservation"){
    tbb::flow::graph g;

    tbb::flow::queue_node<int> q(g);

    bool res = q.try_put(42);
    CHECK_MESSAGE( res, "queue_node must accept input." );

    int val = 1;
    res = q.try_reserve(val);
    CHECK_MESSAGE( res, "queue_node must reserve as it has an item." );
    CHECK_MESSAGE( (val == 42), "queue_node must reserve once passed item." );

    int out_arg = -1;
    CHECK_MESSAGE((q.try_reserve(out_arg) == false), "Reserving a reserved node should fail.");
    CHECK_MESSAGE((out_arg == -1), "Reserving a reserved node should not update its argument.");

    out_arg = -1;
    CHECK_MESSAGE((q.try_get(out_arg) == false), "Getting from reserved node should fail.");
    CHECK_MESSAGE((out_arg == -1), "Getting from reserved node should not update its argument.");
    g.wait_for_all();
}

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
//! \brief \ref error_guessing
TEST_CASE("test queue_node try_put_and_wait") {
    test_queue_node_try_put_and_wait();
}
#endif
