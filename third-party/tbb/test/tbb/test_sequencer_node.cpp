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
#include "common/utils_assert.h"
#include "common/test_follows_and_precedes_api.h"
#include "common/concepts_common.h"

#include <cstdio>
#include <atomic>

#include "test_buffering_try_put_and_wait.h"

//! \file test_sequencer_node.cpp
//! \brief Test for [flow_graph.sequencer_node] specification


#define N 1000
#define C 10

template< typename T >
struct seq_inspector {
    size_t operator()(const T &v) const { return size_t(v); }
};

template< typename T >
bool wait_try_get( tbb::flow::graph &g, tbb::flow::sequencer_node<T> &q, T &value ) {
    g.wait_for_all();
    return q.try_get(value);
}

template< typename T >
void spin_try_get( tbb::flow::queue_node<T> &q, T &value ) {
    while ( q.try_get(value) != true ) ;
}

template< typename T >
struct parallel_puts : utils::NoAssign {

    tbb::flow::sequencer_node<T> &my_q;
    int my_num_threads;

    parallel_puts( tbb::flow::sequencer_node<T> &q, int num_threads ) : my_q(q), my_num_threads(num_threads) {}

    void operator()(int tid) const {
        for (int j = tid; j < N; j+=my_num_threads) {
            bool msg = my_q.try_put( T(j) );
            CHECK_MESSAGE( msg == true, "" );
        }
    }

};

template< typename T >
struct touches {

    bool **my_touches;
    T *my_last_touch;
    int my_num_threads;

    touches( int num_threads ) : my_num_threads(num_threads) {
        my_last_touch = new T[my_num_threads];
        my_touches = new bool* [my_num_threads];
        for ( int p = 0; p < my_num_threads; ++p) {
            my_last_touch[p] = T(-1);
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
        delete [] my_last_touch;
    }

    bool check( int tid, T v ) {
        if ( my_touches[tid][v] != false ) {
            printf("Error: value seen twice by local thread\n");
            return false;
        }
        if ( v <= my_last_touch[tid] ) {
            printf("Error: value seen in wrong order by local thread\n");
            return false;
        }
        my_last_touch[tid] = v;
        my_touches[tid][v] = true;
        return true;
    }

    bool validate_touches() {
        bool *all_touches = new bool[N];
        for ( int n = 0; n < N; ++n)
            all_touches[n] = false;

        for ( int p = 0; p < my_num_threads; ++p) {
            for ( int n = 0; n < N; ++n) {
                if ( my_touches[p][n] == true ) {
                    CHECK_MESSAGE( ( all_touches[n] == false), "value see by more than one thread\n" );
                    all_touches[n] = true;
                }
            }
        }
        for ( int n = 0; n < N; ++n) {
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

    tbb::flow::sequencer_node<T> &my_q;
    int my_num_threads;
    touches<T> &my_touches;

    parallel_gets( tbb::flow::sequencer_node<T> &q, int num_threads, touches<T> &t ) : my_q(q), my_num_threads(num_threads), my_touches(t) {}

    void operator()(int tid) const {
        for (int j = tid; j < N; j+=my_num_threads) {
            T v;
            spin_try_get( my_q, v );
            my_touches.check( tid, v );
        }
    }

};

template< typename T >
struct parallel_put_get : utils::NoAssign {

    tbb::flow::sequencer_node<T> &my_s1;
    tbb::flow::sequencer_node<T> &my_s2;
    int my_num_threads;
    std::atomic< int > &my_counter;
    touches<T> &my_touches;

    parallel_put_get( tbb::flow::sequencer_node<T> &s1, tbb::flow::sequencer_node<T> &s2, int num_threads,
                      std::atomic<int> &counter, touches<T> &t ) : my_s1(s1), my_s2(s2), my_num_threads(num_threads), my_counter(counter), my_touches(t) {}

    void operator()(int tid) const {
        int i_start = 0;

        while ( (i_start = my_counter.fetch_add(C)) < N ) {
            int i_end = ( N < i_start + C ) ? N : i_start + C;
            for (int i = i_start; i < i_end; ++i) {
                bool msg = my_s1.try_put( T(i) );
                CHECK_MESSAGE( msg == true, "" );
            }

            for (int i = i_start; i < i_end; ++i) {
                T v;
                spin_try_get( my_s2, v );
                my_touches.check( tid, v );
            }
        }
    }

};

//
// Tests
//
// multiple parallel senders, multiple receivers, properly sequenced (relative to receiver) at output
// chained sequencers, multiple parallel senders, multiple receivers, properly sequenced (relative to receiver) at output
//

template< typename T >
int test_parallel(int num_threads) {
    tbb::flow::graph g;

    tbb::flow::sequencer_node<T> s(g, seq_inspector<T>());
    utils::NativeParallelFor( num_threads, parallel_puts<T>(s, num_threads) );
    {
        touches<T> t( num_threads );
        utils::NativeParallelFor( num_threads, parallel_gets<T>(s, num_threads, t) );
        g.wait_for_all();
        CHECK_MESSAGE( t.validate_touches(), "" );
    }
    T bogus_value(-1);
    T j = bogus_value;
    CHECK_MESSAGE( s.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    g.wait_for_all();

    tbb::flow::sequencer_node<T> s1(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s2(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s3(g, seq_inspector<T>());
    tbb::flow::make_edge( s1, s2 );
    tbb::flow::make_edge( s2, s3 );

    {
        touches<T> t( num_threads );
        std::atomic<int> counter;
        counter = 0;
        utils::NativeParallelFor( num_threads, parallel_put_get<T>(s1, s3, num_threads, counter, t) );
        g.wait_for_all();
        t.validate_touches();
    }
    g.wait_for_all();
    CHECK_MESSAGE( s1.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( s2.try_get( j ) == false, "" );
    g.wait_for_all();
    CHECK_MESSAGE( s3.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    // test copy constructor
    tbb::flow::sequencer_node<T> s_copy(s);
    utils::NativeParallelFor( num_threads, parallel_puts<T>(s_copy, num_threads) );
    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        spin_try_get( s_copy, j );
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( s_copy.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    return 0;
}


//
// Tests
//
// No predecessors can be registered
// Request from empty buffer fails
// In-order puts, single sender, single receiver, properly sequenced at output
// Reverse-order puts, single sender, single receiver, properly sequenced at output
// Chained sequencers (3), in-order and reverse-order tests, properly sequenced at output
//

template< typename T >
int test_serial() {
    tbb::flow::graph g;
    T bogus_value(-1);

    tbb::flow::sequencer_node<T> s(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s2(g, seq_inspector<T>());
    T j = bogus_value;

    //
    // Rejects attempts to add / remove predecessor
    // Rejects request from empty Q
    //
    CHECK_MESSAGE( register_predecessor( s, s2 ) == false, "" );
    CHECK_MESSAGE( remove_predecessor( s, s2 ) == false, "" );
    CHECK_MESSAGE( s.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    //
    // In-order simple puts and gets
    //

    for (int i = 0; i < N; ++i) {
        bool msg = s.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
        CHECK_MESSAGE(!s.try_put( T(i) ), "");  // second attempt to put should reject
    }


    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        CHECK_MESSAGE(wait_try_get( g, s, j ) == true, "");
        CHECK_MESSAGE( i == j, "" );
        CHECK_MESSAGE(!s.try_put( T(i) ),"" );  // after retrieving value, subsequent put should fail
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( s.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    //
    // Reverse-order simple puts and gets
    //

    for (int i = N-1; i >= 0; --i) {
        bool msg = s2.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        CHECK_MESSAGE(wait_try_get( g, s2, j ) == true, "");
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    g.wait_for_all();
    CHECK_MESSAGE( s2.try_get( j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    //
    // Chained in-order simple puts and gets
    //

    tbb::flow::sequencer_node<T> s3(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s4(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s5(g, seq_inspector<T>());
    tbb::flow::make_edge( s3, s4 );
    tbb::flow::make_edge( s4, s5 );

    for (int i = 0; i < N; ++i) {
        bool msg = s3.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        CHECK_MESSAGE(wait_try_get( g, s5, j ) == true, "");
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    CHECK_MESSAGE( wait_try_get( g, s3, j ) == false, "" );
    CHECK_MESSAGE( wait_try_get( g, s4, j ) == false, "" );
    CHECK_MESSAGE( wait_try_get( g, s5, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    g.wait_for_all();
    tbb::flow::remove_edge( s3, s4 );
    CHECK_MESSAGE( s3.try_put( N ) == true, "" );
    CHECK_MESSAGE( wait_try_get( g, s4, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( wait_try_get( g, s5, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( wait_try_get( g, s3, j ) == true, "" );
    CHECK_MESSAGE( j == N, "" );

    //
    // Chained reverse-order simple puts and gets
    //

    tbb::flow::sequencer_node<T> s6(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s7(g, seq_inspector<T>());
    tbb::flow::sequencer_node<T> s8(g, seq_inspector<T>());
    tbb::flow::make_edge( s6, s7 );
    tbb::flow::make_edge( s7, s8 );

    for (int i = N-1; i >= 0; --i) {
        bool msg = s6.try_put( T(i) );
        CHECK_MESSAGE( msg == true, "" );
    }

    for (int i = 0; i < N; ++i) {
        j = bogus_value;
        CHECK_MESSAGE( wait_try_get( g, s8, j ) == true, "" );
        CHECK_MESSAGE( i == j, "" );
    }
    j = bogus_value;
    CHECK_MESSAGE( wait_try_get( g, s6, j ) == false, "" );
    CHECK_MESSAGE( wait_try_get( g, s7, j ) == false, "" );
    CHECK_MESSAGE( wait_try_get( g, s8, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );

    g.wait_for_all();
    tbb::flow::remove_edge( s6, s7 );
    CHECK_MESSAGE( s6.try_put( N ) == true, "" );
    CHECK_MESSAGE( wait_try_get( g, s7, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( wait_try_get( g, s8, j ) == false, "" );
    CHECK_MESSAGE( j == bogus_value, "" );
    CHECK_MESSAGE( wait_try_get( g, s6, j ) == true, "" );
    CHECK_MESSAGE( j == N, "" );

    return 0;
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    std::array<int, 3> messages_for_follows = { {0, 1, 2} };
    std::vector<int> messages_for_precedes = {0, 1, 2};

    follows_and_precedes_testing::test_follows
        <int, tbb::flow::sequencer_node<int>>
        (messages_for_follows, [](const int& i) -> std::size_t { return i; });

    follows_and_precedes_testing::test_precedes
        <int, tbb::flow::sequencer_node<int>>
        (messages_for_precedes, [](const int& i) -> std::size_t { return i; });
}
#endif

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

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
void test_seq_node_try_put_and_wait() {
    using namespace test_try_put_and_wait;

    std::vector<int> start_work_items;
    std::vector<int> new_work_items;
    int wait_message = 10;

    for (int i = 0; i < wait_message; ++i) {
        start_work_items.emplace_back(i);
        new_work_items.emplace_back(i + 1 + wait_message);
    }

    auto simple_sequencer = [](int item) { return item; };

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

        std::size_t after_start = test_buffer_push<tbb::flow::sequencer_node<int>>(start_work_items, wait_message,
                                                                                   new_work_items, processed_items,
                                                                                   simple_sequencer);

        // Expected effect:
        // During buffer1.try_put_and_wait()
        //     1. start_work_items would be pushed to buffer1
        //     2. wait_message would be pushed to buffer1
        //     3. forward_task on buffer1 would transfer all of the items to the function_node in sequencer order (FIFO)
        //     4. the first item would occupy concurrency of function, other items would be pushed to the queue
        //     5. function would process start_work_items and push them to the buffer2
        //     6. wait_message would be processed last and add new_work_items to buffer1
        //     7. forward_task on buffer2 would transfer start_work_items in sequencer (FIFO) order and the wait_message to the writer
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

        std::size_t after_start = test_buffer_pull<tbb::flow::sequencer_node<int>>(start_work_items, wait_message, occupier,
                                                                                   new_work_items, processed_items,
                                                                                   simple_sequencer);

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

            std::size_t after_start = test_buffer_reserve<tbb::flow::sequencer_node<int>>(threshold,
                start_work_items, wait_message, new_work_items, processed_items, simple_sequencer);

            // Expected effect:
            // 1. start_work_items would be pushed to the buffer
            // 2. wait_message_would be pushed to the buffer
            // 3. forward task of the buffer would push the first message to the limiter node.
            //    Since the limiter threshold is not reached, it would be directly passed to the function
            // 4. function would spawn the task for the first message processing
            // 5. the first would be processed
            // 6. decrementer.try_put() would be called and the limiter node would
            //    process all of the items from the buffer using the try_reserve/try_consume/try_release semantics
            // 7. When the wait_message would be taken from the buffer, the try_put_and_wait would exit

            std::size_t check_index = 0;

            CHECK_MESSAGE(after_start == start_work_items.size() + 1,
                      "start_work_items, occupier and wait_message should be processed by try_put_and_wait");
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
    }
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT

//! Test sequencer with various request orders and parallelism levels
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

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test decution guides
//! \brief \ref requirement
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

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for sequencer_node object") {
    struct Object : test_concepts::Copyable, test_concepts::CopyAssignable {};

    static_assert(utils::well_formed_instantiation<tbb::flow::sequencer_node, Object>);
    static_assert(utils::well_formed_instantiation<tbb::flow::sequencer_node, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::sequencer_node, test_concepts::NonCopyable>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::sequencer_node, test_concepts::NonCopyAssignable>);
}

template <typename T, typename Sequencer>
concept can_call_sequencer_node_ctor = requires( tbb::flow::graph& graph, Sequencer seq,
                                                 tbb::flow::buffer_node<int>& f ) {
    tbb::flow::sequencer_node<T>(graph, seq);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::sequencer_node<T>(tbb::flow::follows(f), seq);
#endif
};

//! \brief \ref error_guessing
TEST_CASE("constraints for sequencer_node sequencer") {
    using type = int;
    using namespace test_concepts::sequencer;

    static_assert(can_call_sequencer_node_ctor<type, Correct<type>>);
    static_assert(!can_call_sequencer_node_ctor<type, NonCopyable<type>>);
    static_assert(!can_call_sequencer_node_ctor<type, NonDestructible<type>>);
    static_assert(!can_call_sequencer_node_ctor<type, NoOperatorRoundBrackets<type>>);
    static_assert(!can_call_sequencer_node_ctor<type, WrongInputOperatorRoundBrackets<type>>);
    static_assert(!can_call_sequencer_node_ctor<type, WrongReturnOperatorRoundBrackets<type>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
//! \brief \ref error_guessing
TEST_CASE("test sequencer_node try_put_and_wait") {
    test_seq_node_try_put_and_wait();
}
#endif
