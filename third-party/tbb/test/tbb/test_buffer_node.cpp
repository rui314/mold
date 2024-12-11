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

#include "common/config.h"

#include "tbb/flow_graph.h"
#include "tbb/global_control.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"

#include "test_buffering_try_put_and_wait.h"

//! \file test_buffer_node.cpp
//! \brief Test for [flow_graph.buffer_node] specification

#define N 1000
#define C 10

template< typename T >
void spin_try_get( tbb::flow::buffer_node<T> &b, T &value ) {
    while ( b.try_get(value) != true ) {}
}

template< typename T >
void check_item( T* count_value, T &value ) {
    count_value[value / N] += value % N;
}

template< typename T >
struct parallel_puts : utils::NoAssign {

    tbb::flow::buffer_node<T> &my_b;

    parallel_puts( tbb::flow::buffer_node<T> &b ) : my_b(b) {}

    void operator()(int i) const {
        for (int j = 0; j < N; ++j) {
            bool msg = my_b.try_put( T(N*i + j) );
            CHECK_MESSAGE( msg == true, "" );
        }
    }
};

template< typename T >
struct touches {

    bool **my_touches;
    int my_num_threads;

    touches( int num_threads ) : my_num_threads(num_threads) {
        my_touches = new bool* [my_num_threads];
        for ( int p = 0; p < my_num_threads; ++p) {
            my_touches[p] = new bool[N];
            for ( int n = 0; n < N; ++n)
                my_touches[p][n] = false;
        }
    }

    ~touches() {
        for ( int p = 0; p < my_num_threads; ++p) {
            delete [] my_touches[p];
        }
        delete [] my_touches;
    }

    bool check( T v ) {
        CHECK_MESSAGE( my_touches[v/N][v%N] == false, "" );
        my_touches[v/N][v%N] = true;
        return true;
    }

    bool validate_touches() {
        for ( int p = 0; p < my_num_threads; ++p) {
            for ( int n = 0; n < N; ++n) {
                CHECK_MESSAGE( my_touches[p][n] == true, "" );
            }
        }
        return true;
    }
};

template< typename T >
struct parallel_gets : utils::NoAssign {

    tbb::flow::buffer_node<T> &my_b;
    touches<T> &my_touches;

    parallel_gets( tbb::flow::buffer_node<T> &b, touches<T> &t) : my_b(b), my_touches(t) {}

    void operator()(int) const {
        for (int j = 0; j < N; ++j) {
            T v;
            spin_try_get( my_b, v );
            my_touches.check( v );
        }
    }

};

template< typename T >
struct parallel_put_get : utils::NoAssign {

    tbb::flow::buffer_node<T> &my_b;
    touches<T> &my_touches;

    parallel_put_get( tbb::flow::buffer_node<T> &b, touches<T> &t ) : my_b(b), my_touches(t) {}

    void operator()(int tid) const {

        for ( int i = 0; i < N; i+=C ) {
            int j_end = ( N < i + C ) ? N : i + C;
            // dump about C values into the buffer
            for ( int j = i; j < j_end; ++j ) {
                CHECK_MESSAGE( my_b.try_put( T (N*tid + j ) ) == true, "" );
            }
            // receiver about C values from the buffer
            for ( int j = i; j < j_end; ++j ) {
                T v;
                spin_try_get( my_b, v );
                my_touches.check( v );
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
    tbb::flow::buffer_node<T> b(g);

    b.try_put(T(1));
    b.try_put(T(2));
    b.try_put(T(3));

    T v, vsum;
    CHECK_MESSAGE( b.try_reserve(v) == true, "" );
    CHECK_MESSAGE( b.try_release() == true, "" );
    v = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_reserve(v) == true, "" );
    CHECK_MESSAGE( b.try_consume() == true, "" );
    vsum += v;
    v = bogus_value;
    g.wait_for_all();

    CHECK_MESSAGE( b.try_get(v) == true, "" );
    vsum += v;
    v = bogus_value;
    g.wait_for_all();

    CHECK_MESSAGE( b.try_reserve(v) == true, "" );
    CHECK_MESSAGE( b.try_release() == true, "" );
    v = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_reserve(v) == true, "" );
    CHECK_MESSAGE( b.try_consume() == true, "" );
    vsum += v;
    CHECK_MESSAGE( vsum == T(6), "");
    v = bogus_value;
    g.wait_for_all();

    return 0;
}

//
// Tests
//
// multiple parallel senders, items in arbitrary order
// multiple parallel senders, multiple parallel receivers, items in arbitrary order and all items received
//   * overlapped puts / gets
//   * all puts finished before any getS
//
template< typename T >
int test_parallel(int num_threads) {
    tbb::flow::graph g;
    tbb::flow::buffer_node<T> b(g);
    tbb::flow::buffer_node<T> b2(g);
    tbb::flow::buffer_node<T> b3(g);
    T bogus_value(-1);
    T j = bogus_value;

    NativeParallelFor( num_threads, parallel_puts<T>(b) );

    T *next_value = new T[num_threads];
    for (int tid = 0; tid < num_threads; ++tid) next_value[tid] = T(0);

    for (int i = 0; i < num_threads * N; ++i ) {
        spin_try_get( b, j );
        check_item( next_value, j );
        j = bogus_value;
    }
    for (int tid = 0; tid < num_threads; ++tid)  {
        CHECK_MESSAGE( next_value[tid] == T((N*(N-1))/2), "" );
    }

    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    NativeParallelFor( num_threads, parallel_puts<T>(b) );

    {
        touches< T > t( num_threads );
        NativeParallelFor( num_threads, parallel_gets<T>(b, t) );
        g.wait_for_all();
        CHECK_MESSAGE( t.validate_touches(), "" );
    }
    j = bogus_value;
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    g.wait_for_all();
    {
        touches< T > t( num_threads );
        NativeParallelFor( num_threads, parallel_put_get<T>(b, t) );
        g.wait_for_all();
        CHECK_MESSAGE( t.validate_touches(), "" );
    }
    j = bogus_value;
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::make_edge( b, b2 );
    tbb::flow::make_edge( b2, b3 );

    NativeParallelFor( num_threads, parallel_puts<T>(b) );
    {
        touches< T > t( num_threads );
        NativeParallelFor( num_threads, parallel_gets<T>(b3, t) );
        g.wait_for_all();
        CHECK_MESSAGE( t.validate_touches(), "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b2.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    // test copy constructor
    CHECK_MESSAGE( b.remove_successor( b2 ), "" );
    // fill up b:
    NativeParallelFor( num_threads, parallel_puts<T>(b) );
    // copy b:
    tbb::flow::buffer_node<T> b_copy(b);

    // b_copy should be empty
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b_copy.try_get( j ) == false, "" );

    // hook them together:
    CHECK_MESSAGE( b.register_successor(b_copy) == true, "" );
    // try to get content from b_copy
    {
        touches< T > t( num_threads );
        NativeParallelFor( num_threads, parallel_gets<T>(b_copy, t) );
        g.wait_for_all();
        CHECK_MESSAGE( t.validate_touches(), "" );
    }
    // now both should be empty
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b_copy.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    delete [] next_value;
    return 0;
}

//
// Tests
//
// Predecessors cannot be registered
// Empty buffer rejects item requests
// Single serial sender, items in arbitrary order
// Chained buffers ( 2 & 3 ), single sender, items at last buffer in arbitrary order
//

#define TBB_INTERNAL_NAMESPACE detail::d2
using tbb::TBB_INTERNAL_NAMESPACE::register_predecessor;
using tbb::TBB_INTERNAL_NAMESPACE::remove_predecessor;

template< typename T >
int test_serial() {
    tbb::flow::graph g;
    T bogus_value(-1);

    tbb::flow::buffer_node<T> b(g);
    tbb::flow::buffer_node<T> b2(g);
    T j = bogus_value;

    //
    // Rejects attempts to add / remove predecessor
    // Rejects request from empty buffer
    //
    CHECK_MESSAGE( register_predecessor<T>( b, b2 ) == false, "" );
    CHECK_MESSAGE( remove_predecessor<T>( b, b2 ) == false, "" );
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    //
    // Simple puts and gets
    //

    for (int i = 0; i < N; ++i) {
        bool msg = b.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    T vsum = T(0);
    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        spin_try_get( b, j );
        vsum += j;
    }
    CHECK_MESSAGE( vsum == (N*(N-1))/2, "");
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::make_edge(b, b2);

    vsum = T(0);
    for (int i = 0; i < N; ++i) {
        bool msg = b.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        spin_try_get( b2, j );
        vsum += j;
    }
    CHECK_MESSAGE( vsum == (N*(N-1))/2, "");
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::remove_edge(b, b2);
    CHECK_MESSAGE( b.try_put( 1 ) == true, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == true, "" );
    CHECK_MESSAGE( j == 1, "" );

    tbb::flow::buffer_node<T> b3(g);
    tbb::flow::make_edge( b, b2 );
    tbb::flow::make_edge( b2, b3 );

    vsum = T(0);
    for (int i = 0; i < N; ++i) {
        bool msg = b.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        spin_try_get( b3, j );
        vsum += j;
    }
    CHECK_MESSAGE( vsum == (N*(N-1))/2, "");
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b2.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    tbb::flow::remove_edge(b, b2);
    CHECK_MESSAGE( b.try_put( 1 ) == true, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();
    CHECK_MESSAGE( b.try_get( j ) == true, "" );
    CHECK_MESSAGE( j == 1, "" );

    return 0;
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t(), msg_t(), msg_t()};

    follows_and_precedes_testing::test_follows<msg_t, tbb::flow::buffer_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes<msg_t, tbb::flow::buffer_node<msg_t>>(messages_for_precedes);
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;
    graph g;
    broadcast_node<int> br(g);
    buffer_node<int> b0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    buffer_node b1(follows(br));
    static_assert(std::is_same_v<decltype(b1), buffer_node<int>>);

    buffer_node b2(precedes(br));
    static_assert(std::is_same_v<decltype(b2), buffer_node<int>>);
#endif

    buffer_node b3(b0);
    static_assert(std::is_same_v<decltype(b3), buffer_node<int>>);
    g.wait_for_all();
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
void test_buffer_node_try_put_and_wait() {
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

        std::size_t after_start = test_buffer_push<tbb::flow::buffer_node<int>>(start_work_items, wait_message,
                                                                                new_work_items, processed_items);

        // Expected effect:
        // During buffer1.try_put_and_wait()
        //     1. start_work_items would be pushed to buffer1
        //     2. wait_message would be pushed to buffer1
        //     3. forward_task on buffer1 would transfer all of the items to the function_node in LIFO order
        //     4. wait_message would occupy concurrency of function, other items would be pushed to the queue
        //     5. function would process wait_message and add new_work_items to the buffer1
        //     6. forward_task for new_work_items would be spawned, wait_message would be buffered in the buffer2
        //     7. function task for next FIFO item in the queue would be spawned
        //     8. forward_task for wait_message in buffer2 would be executed without spawning
        //     9. writer task for wait_message would be executed without spawning and write wait_message to the buffer
        //     10. try_put_and_wait exits since wait_message is completed
        // During g.wait_for_all()
        //     10. forward_task for new_work_items in buffer1 would be spawned and put items in function in LIFO order
        //     11. function_node would process and push forward items from the queue in FIFO order
        // Expected items processing - { wait_message, start_work_items LIFO, new_work_items LIFO }

        std::size_t check_index = 0;
        CHECK_MESSAGE(after_start == 1, "try_put_and_wait should process only the wait_message");
        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "try_put_and_wait should process only the wait_message");

        for (std::size_t index = start_work_items.size(); index != 0; --index) {
            CHECK_MESSAGE(processed_items[check_index++] == start_work_items[index - 1],
                          "wait_for_all should process start_work_items LIFO");
        }
        for (std::size_t index = new_work_items.size(); index != 0; --index) {
            CHECK_MESSAGE(processed_items[check_index++] == new_work_items[index - 1],
                          "wait_for_all should process new_work_items LIFO");
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

        std::size_t after_start = test_buffer_pull<tbb::flow::buffer_node<int>>(start_work_items, wait_message, occupier,
                                                                                new_work_items, processed_items);

        // Expected effect
        // 0. task for occupier processing would be spawned by the function
        // During buffer.try_put_and_wait()
        //     1. start_work_items would be pushed to the buffer
        //     2. wait_message would be pushed to the buffer
        //     3. forward_task would try to push items to the function, but would fail
        //        and set the edge to the pull state
        //     4. occupier would be processed
        //     5. items would be taken from the buffer by function in LIFO order
        //     6. wait_message would be taken first and push new_work_items to the buffer
        // Expected items processing { occupier, wait_message, new_work_items LIFO, start_work_items LIFO }

        std::size_t check_index = 0;

        CHECK_MESSAGE(after_start == 2, "Only wait_message and occupier should be processed by try_put_and_wait");
        CHECK_MESSAGE(processed_items[check_index++] == occupier, "Unexpected items processing by try_put_and_wait");
        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected items processing by try_put_and_wait");

        for (std::size_t index = new_work_items.size(); index != 0; --index) {
            CHECK_MESSAGE(processed_items[check_index++] == new_work_items[index - 1],
                          "wait_for_all should process new_work_items LIFO");
        }
        for (std::size_t index = start_work_items.size(); index != 0; --index) {
            CHECK_MESSAGE(processed_items[check_index++] == start_work_items[index - 1],
                          "wait_for_all should process start_work_items LIFO");
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

            std::size_t after_start = test_buffer_reserve<tbb::flow::buffer_node<int>>(threshold,
                start_work_items, wait_message, new_work_items, processed_items);

            // Expected effect:
            // 1. start_work_items would be pushed to the buffer
            // 2. wait_message_would be pushed to the buffer
            // 3. forward task of the buffer would push wait_message to the limiter node.
            //    Since the limiter threshold is not reached, it would be directly passed to the function
            // 4. function would spawn the task for wait_message processing
            // 5. wait_message would be processed that would add new_work_items to the buffer
            // 6. decrementer.try_put() would be called and the limiter node would
            //    process all of the items from the buffer using the try_reserve/try_consume/try_release semantics
            // Since the reservation always accepts the front element of the buffer
            // it is expected that the items would be taken from the buffer in FIFO order
            // instead of LIFO on try_get for buffer_node

            std::size_t check_index = 0;

            CHECK_MESSAGE(after_start == 1, "try_put_and_wait should process only wait_message");
            CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected wait_message processing");

            for (auto item : start_work_items) {
                CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
            }

            for (auto item : new_work_items) {
                CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
            }

        }
    }
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT

#include <iomanip>

//! Test buffer_node with parallel and serial neighbours
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Serial and parallel test"){
    for (int p = 2; p <= 4; ++p) {
        tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, p);
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_serial<int>();
                test_parallel<int>(p);
            }
        );
    }
}

//! Test reset and cancellation behavior
//! \brief \ref error_guessing
TEST_CASE("Resets"){
    test_resets<int,tbb::flow::buffer_node<int> >();
    test_resets<float,tbb::flow::buffer_node<float> >();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test deprecated follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Follows and precedes API"){
    test_follows_and_precedes_api();
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("Deduction guides"){
    test_deduction_guides();
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
//! \brief \ref error_guessing
TEST_CASE("test buffer_node try_put_and_wait") {
    test_buffer_node_try_put_and_wait();
}
#endif
