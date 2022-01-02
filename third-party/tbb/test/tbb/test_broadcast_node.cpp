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

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/test_follows_and_precedes_api.h"

#include <atomic>


//! \file test_broadcast_node.cpp
//! \brief Test for [flow_graph.broadcast_node] specification


#define TBB_INTERNAL_NAMESPACE detail::d1
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
//! Test deprecated follows and preceedes API
//! \brief \ref error_guessing
TEST_CASE("Follows and preceedes API"){
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

