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

#include "common/test.h"
#include "common/utils.h"
#include "common/test_follows_and_precedes_api.h"

#include <atomic>


//! \file test_broadcast_node.cpp
//! \brief Test for [flow_graph.broadcast_node] specification


#define TBB_INTERNAL_NAMESPACE detail::d2
namespace tbb {
using task = TBB_INTERNAL_NAMESPACE::graph_task;
}
using tbb::TBB_INTERNAL_NAMESPACE::SUCCESSFULLY_ENQUEUED;

const int N = 1000;
const int R = 4;

class int_convertable_type : private utils::NoAssign {

   int my_value;

public:

   int_convertable_type( int v ) : my_value(v) {}
   operator int() const { return my_value; }

};


template< typename T >
class counting_array_receiver : public tbb::flow::receiver<T> {

    std::atomic<size_t> my_counters[N];
    tbb::flow::graph& my_graph;

public:

    counting_array_receiver(tbb::flow::graph& g) : my_graph(g) {
        for (int i = 0; i < N; ++i )
           my_counters[i] = 0;
    }

    size_t operator[]( int i ) {
        size_t v = my_counters[i];
        return v;
    }

    tbb::task * try_put_task( const T &v ) override {
        ++my_counters[(int)v];
        return const_cast<tbb::task *>(SUCCESSFULLY_ENQUEUED);
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    tbb::task * try_put_task( const T &v, const tbb::detail::d2::message_metainfo& ) override {
        return try_put_task(v);
    }
#endif

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }
};

template< typename T >
void test_serial_broadcasts() {

    tbb::flow::graph g;
    tbb::flow::broadcast_node<T> b(g);

    for ( int num_receivers = 1; num_receivers < R; ++num_receivers ) {
        std::vector< std::shared_ptr<counting_array_receiver<T>> > receivers;
        for( int i = 0; i < num_receivers; ++i )
            receivers.push_back( std::make_shared<counting_array_receiver<T>>(g) );

        for ( int r = 0; r < num_receivers; ++r ) {
            tbb::flow::make_edge( b, *receivers[r] );
        }

        for (int n = 0; n < N; ++n ) {
            CHECK_MESSAGE( b.try_put( (T)n ), "" );
        }

        for ( int r = 0; r < num_receivers; ++r ) {
            for (int n = 0; n < N; ++n ) {
                CHECK_MESSAGE( (*receivers[r])[n] == 1, "" );
            }
            tbb::flow::remove_edge( b, *receivers[r] );
        }
        CHECK_MESSAGE( b.try_put( (T)0 ), "" );
        for ( int r = 0; r < num_receivers; ++r )
            CHECK_MESSAGE( (*receivers[0])[0] == 1, "" );
    }

}

template< typename T >
class native_body : private utils::NoAssign {

    tbb::flow::broadcast_node<T> &my_b;

public:

    native_body( tbb::flow::broadcast_node<T> &b ) : my_b(b) {}

    void operator()(int) const {
        for (int n = 0; n < N; ++n ) {
            CHECK_MESSAGE( my_b.try_put( (T)n ), "" );
        }
    }

};

template< typename T >
void run_parallel_broadcasts(tbb::flow::graph& g, int p, tbb::flow::broadcast_node<T>& b) {
    for ( int num_receivers = 1; num_receivers < R; ++num_receivers ) {
        std::vector< std::shared_ptr<counting_array_receiver<T>> > receivers;
        for( int i = 0; i < num_receivers; ++i )
            receivers.push_back( std::make_shared< counting_array_receiver<T> >(g) );

        for ( int r = 0; r < num_receivers; ++r ) {
            tbb::flow::make_edge( b, *receivers[r] );
        }

        utils::NativeParallelFor( p, native_body<T>( b ) );

        for ( int r = 0; r < num_receivers; ++r ) {
            for (int n = 0; n < N; ++n ) {
                CHECK_MESSAGE( (int)(*receivers[r])[n] == p, "" );
            }
            tbb::flow::remove_edge( b, *receivers[r] );
        }
        CHECK_MESSAGE( b.try_put( (T)0 ), "" );
        for ( int r = 0; r < num_receivers; ++r )
            CHECK_MESSAGE( (int)(*receivers[r])[0] == p, "" );
    }
}

template< typename T >
void test_parallel_broadcasts(int p) {

    tbb::flow::graph g;
    tbb::flow::broadcast_node<T> b(g);
    run_parallel_broadcasts(g, p, b);

    // test copy constructor
    tbb::flow::broadcast_node<T> b_copy(b);
    run_parallel_broadcasts(g, p, b_copy);
}

// broadcast_node does not allow successors to try_get from it (it does not allow
// the flow edge to switch) so we only need test the forward direction.
template<typename T>
void test_resets() {
    tbb::flow::graph g;
    tbb::flow::broadcast_node<T> b0(g);
    tbb::flow::broadcast_node<T> b1(g);
    tbb::flow::queue_node<T> q0(g);
    tbb::flow::make_edge(b0,b1);
    tbb::flow::make_edge(b1,q0);
    T j;

    // test standard reset
    for(int testNo = 0; testNo < 2; ++testNo) {
        for(T i= 0; i <= 3; i += 1) {
            b0.try_put(i);
        }
        g.wait_for_all();
        for(T i= 0; i <= 3; i += 1) {
            CHECK_MESSAGE( (q0.try_get(j) && j == i), "Bad value in queue");
        }
        CHECK_MESSAGE( (!q0.try_get(j)), "extra value in queue");

        // reset the graph.  It should work as before.
        if (testNo == 0) g.reset();
    }

    g.reset(tbb::flow::rf_clear_edges);
    for(T i= 0; i <= 3; i += 1) {
        b0.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( (!q0.try_get(j)), "edge between nodes not removed");
    for(T i= 0; i <= 3; i += 1) {
        b1.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( (!q0.try_get(j)), "edge between nodes not removed");
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows= { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t()};

    follows_and_precedes_testing::test_follows <msg_t, tbb::flow::broadcast_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes <msg_t, tbb::flow::broadcast_node<msg_t>>(messages_for_precedes);
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;

    broadcast_node<int> b0(g);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    buffer_node<int> buf(g);

    broadcast_node b1(follows(buf));
    static_assert(std::is_same_v<decltype(b1), broadcast_node<int>>);

    broadcast_node b2(precedes(buf));
    static_assert(std::is_same_v<decltype(b2), broadcast_node<int>>);
#endif

    broadcast_node b3(b0);
    static_assert(std::is_same_v<decltype(b3), broadcast_node<int>>);
    g.wait_for_all();
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
// Basic idea of the following tests is to check that try_put_and_wait(message) call for broadcast_node
// processes all of the previous jobs required to process message, the message itself, but does
// not process the elements submitted later or not required to process the message
// These tests submit start_work_items using the regular try_put and then submit wait_message
// with try_put_and_wait. During the completion of the graph, new_work_items would be submitted
// once the wait_message arrives.
void test_try_put_and_wait_spawning_and_serial_receiver() {
    tbb::task_arena arena(1);

    arena.execute([&]{
        tbb::flow::graph g;

        std::vector<int> start_work_items;
        std::vector<int> processed_items_unlimited, processed_items_serial;
        std::vector<int> new_work_items;

        int wait_message = 10;

        for (int i = 0; i < wait_message; ++i) {
            start_work_items.emplace_back(i);
            new_work_items.emplace_back(i + 1 + wait_message);
        }

        tbb::flow::broadcast_node<int> broadcast(g);

        // Broadcast to 2 function_nodes, one with unlimited concurrency and the other serial
        tbb::flow::function_node<int, int, tbb::flow::queueing> unlimited(g, tbb::flow::unlimited,
            [&](int input) noexcept {
                if (input == wait_message) {
                    for (auto item : new_work_items) {
                        broadcast.try_put(item);
                    }
                }
                processed_items_unlimited.emplace_back(input);
                return 0;
            });
        tbb::flow::make_edge(broadcast, unlimited);

        tbb::flow::function_node<int, int, tbb::flow::queueing> serial(g, tbb::flow::serial,
            [&](int input) noexcept {
                processed_items_serial.emplace_back(input);
                return 0;
            });
        tbb::flow::make_edge(broadcast, serial);

        for (int i = 0; i < wait_message; ++i) {
            broadcast.try_put(i);
        }

        broadcast.try_put_and_wait(wait_message);

        size_t unlimited_check_index = 0, serial_check_index = 0;

        // For the unlimited function_node, all of the tasks for start_work_items and wait_message would be spawned
        // and hence processed by the thread in LIFO order.
        // The first processed item is expected to be wait_message since it was spawned last
        CHECK_MESSAGE(processed_items_unlimited.size() == new_work_items.size() + start_work_items.size(),
                      "Unexpected number of processed items");
        CHECK_MESSAGE(processed_items_unlimited[unlimited_check_index++] == wait_message, "Unexpected items processing");
        for (int i = int(new_work_items.size()) - 1; i >= 0; --i) {
            CHECK_MESSAGE(processed_items_unlimited[unlimited_check_index++] == new_work_items[i], "Unexpected items processing");
        }
        for (int i = int(start_work_items.size()) - 1; i >= 1; --i) {
            CHECK_MESSAGE(processed_items_unlimited[unlimited_check_index++] == start_work_items[i], "Unexpected items processing");
        }

        // Serial queueing function_node should add all start_work_items except the first one into the queue
        // and then process them in FIFO order.
        // wait_message would also be added to the queue, but would be processed later
        CHECK_MESSAGE(processed_items_serial.size() == start_work_items.size() + 1,
                      "Unexpected number of processed items");
        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items_serial[serial_check_index++] == item, "Unexpected items processing");
        }
        CHECK_MESSAGE(processed_items_serial[serial_check_index++] == wait_message, "Unexpected items processing");

        g.wait_for_all();

        CHECK_MESSAGE(processed_items_unlimited[unlimited_check_index++] == start_work_items[0], "Unexpected items processing");

        // For serial queueing function_node, the new_work_items are expected to be processed while calling to wait_for_all
        // They would be queued and processed later in FIFO order
        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items_serial[serial_check_index++] == item, "Unexpected items processing");
        }
        CHECK(serial_check_index == processed_items_serial.size());
        CHECK(unlimited_check_index == processed_items_unlimited.size());
    });
}

void test_try_put_and_wait_spawning_receivers() {
    tbb::task_arena arena(1);

    arena.execute([&]{
        tbb::flow::graph g;

        int wait_message = 10;
        int num_successors = wait_message - 1;

        std::vector<int> start_work_items;
        std::vector<std::vector<int>> processed_items(num_successors);
        std::vector<int> new_work_items;

        for (int i = 0; i < wait_message; ++i) {
            start_work_items.emplace_back(i);
            new_work_items.emplace_back(i + 1 + wait_message);
        }

        tbb::flow::broadcast_node<int> broadcast(g);

        std::vector<tbb::flow::function_node<int, int, tbb::flow::queueing>> successors;
        successors.reserve(num_successors);
        for (int i = 0; i < num_successors; ++i) {
            successors.emplace_back(g, tbb::flow::unlimited,
                [&, i](int input) noexcept {
                    if (input == wait_message) {
                        broadcast.try_put(new_work_items[i]);
                    }
                    processed_items[i].emplace_back(input);
                    return 0;
                });
            tbb::flow::make_edge(broadcast, successors.back());
        }

        for (int i = 0; i < wait_message; ++i) {
            broadcast.try_put(i);
        }

        broadcast.try_put_and_wait(wait_message);

        for (int i = num_successors - 1; i >= 0; --i) {
            size_t check_index = 0;
            for (int j = num_successors - 1; j != i; --j) {
                CHECK_MESSAGE(processed_items[i][check_index++] == new_work_items[j], "Unexpected items processing");
            }
            CHECK_MESSAGE(processed_items[i][check_index++] == wait_message, "Unexpected items processing");
            for (int j = i; j >= 1; --j) {
                CHECK_MESSAGE(processed_items[i][check_index++] == new_work_items[j], "Unexpected items processing");
            }
        }

        g.wait_for_all();

        for (auto& processed_item : processed_items) {
            size_t check_index = num_successors;
            CHECK_MESSAGE(processed_item[check_index++] == new_work_items[0], "Unexpected items processing");
            for (int i = int(start_work_items.size()) - 1; i >= 0; --i) {
                CHECK_MESSAGE(processed_item[check_index++] == start_work_items[i], "Unexpected items processing");
            }
        }
    });
}

void test_try_put_and_wait() {
    test_try_put_and_wait_spawning_and_serial_receiver();
    test_try_put_and_wait_spawning_receivers();
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT

//! Test serial broadcasts
//! \brief \ref error_guessing
TEST_CASE("Serial broadcasts"){
   test_serial_broadcasts<int>();
   test_serial_broadcasts<float>();
   test_serial_broadcasts<int_convertable_type>();
}

//! Test parallel broadcasts
//! \brief \ref error_guessing
TEST_CASE("Parallel broadcasts"){
    for( unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
       test_parallel_broadcasts<int>(p);
       test_parallel_broadcasts<float>(p);
       test_parallel_broadcasts<int_convertable_type>(p);
   }
}

//! Test reset and cancellation behavior
//! \brief \ref error_guessing
TEST_CASE("Resets"){
   test_resets<int>();
   test_resets<float>();
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
//! \brief requirement
TEST_CASE("Deduction guides"){
    test_deduction_guides();
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
//! \brief \ref error_guessing
TEST_CASE("test broadcast_node try_put_and_wait") {
    test_try_put_and_wait();
}
#endif
