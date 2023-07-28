/*
    Copyright (c) 2005-2022 Intel Corporation

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

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/test_follows_and_precedes_api.h"
#include "tbb/global_control.h"

#include <atomic>


//! \file test_limiter_node.cpp
//! \brief Test for [flow_graph.limiter_node] specification


const int L = 10;
const int N = 1000;

using tbb::detail::d1::SUCCESSFULLY_ENQUEUED;
using tbb::detail::d1::graph_task;

template< typename T >
struct serial_receiver : public tbb::flow::receiver<T>, utils::NoAssign {
   T next_value;
   tbb::flow::graph& my_graph;

   serial_receiver(tbb::flow::graph& g) : next_value(T(0)), my_graph(g) {}

   graph_task* try_put_task( const T &v ) override {
       CHECK_MESSAGE( next_value++  == v, "" );
       return const_cast<graph_task*>(SUCCESSFULLY_ENQUEUED);
   }

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }
};

template< typename T >
struct parallel_receiver : public tbb::flow::receiver<T>, utils::NoAssign {

    std::atomic<int> my_count;
    tbb::flow::graph& my_graph;

    parallel_receiver(tbb::flow::graph& g) : my_graph(g) { my_count = 0; }

    graph_task* try_put_task( const T &/*v*/ ) override {
       ++my_count;
       return const_cast<graph_task*>(SUCCESSFULLY_ENQUEUED);
    }

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }
};

template< typename T >
struct empty_sender : public tbb::flow::sender<T> {
        typedef typename tbb::flow::sender<T>::successor_type successor_type;

        bool register_successor( successor_type & ) override { return false; }
        bool remove_successor( successor_type & ) override { return false; }
};


template< typename T >
struct put_body : utils::NoAssign {

    tbb::flow::limiter_node<T> &my_lim;
    std::atomic<int> &my_accept_count;

    put_body( tbb::flow::limiter_node<T> &lim, std::atomic<int> &accept_count ) :
        my_lim(lim), my_accept_count(accept_count) {}

    void operator()( int ) const {
        for ( int i = 0; i < L; ++i ) {
            bool msg = my_lim.try_put( T(i) );
            if ( msg == true )
               ++my_accept_count;
        }
    }
};

template< typename T >
struct put_dec_body : utils::NoAssign {

    tbb::flow::limiter_node<T> &my_lim;
    std::atomic<int> &my_accept_count;

    put_dec_body( tbb::flow::limiter_node<T> &lim, std::atomic<int> &accept_count ) :
        my_lim(lim), my_accept_count(accept_count) {}

    void operator()( int ) const {
        int local_accept_count = 0;
        while ( local_accept_count < N ) {
            bool msg = my_lim.try_put( T(local_accept_count) );
            if ( msg == true ) {
                ++local_accept_count;
                ++my_accept_count;
                my_lim.decrementer().try_put( tbb::flow::continue_msg() );
            }
        }
    }

};

template< typename T >
void test_puts_with_decrements( int num_threads, tbb::flow::limiter_node< T >& lim , tbb::flow::graph& g) {
    parallel_receiver<T> r(g);
    empty_sender< tbb::flow::continue_msg > s;
    std::atomic<int> accept_count;
    accept_count = 0;
    tbb::flow::make_edge( lim, r );
    tbb::flow::make_edge(s, lim.decrementer());

    // test puts with decrements
    utils::NativeParallelFor( num_threads, put_dec_body<T>(lim, accept_count) );
    int c = accept_count;
    CHECK_MESSAGE( c == N*num_threads, "" );
    CHECK_MESSAGE( r.my_count == N*num_threads, "" );
}

//
// Tests
//
// limiter only forwards below the limit, multiple parallel senders / single receiver
// multiple parallel senders that put to decrement at each accept, limiter accepts new messages
//
//
template< typename T >
int test_parallel(int num_threads) {

   // test puts with no decrements
   for ( int i = 0; i < L; ++i ) {
       tbb::flow::graph g;
       tbb::flow::limiter_node< T > lim(g, i);
       parallel_receiver<T> r(g);
       std::atomic<int> accept_count;
       accept_count = 0;
       tbb::flow::make_edge( lim, r );
       // test puts with no decrements
       utils::NativeParallelFor( num_threads, put_body<T>(lim, accept_count) );
       g.wait_for_all();
       int c = accept_count;
       CHECK_MESSAGE( c == i, "" );
   }

   // test puts with decrements
   for ( int i = 1; i < L; ++i ) {
       tbb::flow::graph g;
       tbb::flow::limiter_node< T > lim(g, i);
       test_puts_with_decrements(num_threads, lim, g);
       tbb::flow::limiter_node< T > lim_copy( lim );
       test_puts_with_decrements(num_threads, lim_copy, g);
   }

   return 0;
}

//
// Tests
//
// limiter only forwards below the limit, single sender / single receiver
// at reject, a put to decrement, will cause next message to be accepted
//
template< typename T >
int test_serial() {

   // test puts with no decrements
   for ( int i = 0; i < L; ++i ) {
       tbb::flow::graph g;
       tbb::flow::limiter_node< T > lim(g, i);
       serial_receiver<T> r(g);
       tbb::flow::make_edge( lim, r );
       for ( int j = 0; j < L; ++j ) {
           bool msg = lim.try_put( T(j) );
           CHECK_MESSAGE( (( j < i && msg == true ) || ( j >= i && msg == false )), "" );
       }
       g.wait_for_all();
   }

   // test puts with decrements
   for ( int i = 1; i < L; ++i ) {
       tbb::flow::graph g;
       tbb::flow::limiter_node< T > lim(g, i);
       serial_receiver<T> r(g);
       empty_sender< tbb::flow::continue_msg > s;
       tbb::flow::make_edge( lim, r );
       tbb::flow::make_edge(s, lim.decrementer());
       for ( int j = 0; j < N; ++j ) {
           bool msg = lim.try_put( T(j) );
           CHECK_MESSAGE( (( j < i && msg == true ) || ( j >= i && msg == false )), "" );
           if ( msg == false ) {
               lim.decrementer().try_put( tbb::flow::continue_msg() );
               msg = lim.try_put( T(j) );
               CHECK_MESSAGE( msg == true, "" );
           }
       }
   }
   return 0;
}

// reported bug in limiter (http://software.intel.com/en-us/comment/1752355)
#define DECREMENT_OUTPUT 1  // the port number of the decrement output of the multifunction_node
#define LIMITER_OUTPUT 0    // port number of the integer output

typedef tbb::flow::multifunction_node<int, std::tuple<int,tbb::flow::continue_msg> > mfnode_type;

std::atomic<size_t> emit_count;
std::atomic<size_t> emit_sum;
std::atomic<size_t> receive_count;
std::atomic<size_t> receive_sum;

struct mfnode_body {
    int max_cnt;
    std::atomic<int>* my_cnt;
    mfnode_body(const int& _max, std::atomic<int> &_my) : max_cnt(_max), my_cnt(&_my)  { }
    void operator()(const int &/*in*/, mfnode_type::output_ports_type &out) {
        int lcnt = ++(*my_cnt);
        if(lcnt > max_cnt) {
            return;
        }
        // put one continue_msg to the decrement of the limiter.
        if(!std::get<DECREMENT_OUTPUT>(out).try_put(tbb::flow::continue_msg())) {
            CHECK_MESSAGE( (false),"Unexpected rejection of decrement");
        }
        {
            // put messages to the input of the limiter_node until it rejects.
            while( std::get<LIMITER_OUTPUT>(out).try_put(lcnt) ) {
                emit_sum += lcnt;
                ++emit_count;
            }
        }
    }
};

struct fn_body {
    int operator()(const int &in) {
        receive_sum += in;
        ++receive_count;
        return in;
    }
};

//                   +------------+
//    +---------+    |            v
//    | mf_node |0---+       +----------+          +----------+
// +->|         |1---------->| lim_node |--------->| fn_node  |--+
// |  +---------+            +----------+          +----------+  |
// |                                                             |
// |                                                             |
// +-------------------------------------------------------------+
//
void
test_multifunction_to_limiter(int _max, int _nparallel) {
    tbb::flow::graph g;
    emit_count = 0;
    emit_sum = 0;
    receive_count = 0;
    receive_sum = 0;
    std::atomic<int> local_cnt;
    local_cnt = 0;
    mfnode_type mf_node(g, tbb::flow::unlimited, mfnode_body(_max, local_cnt));
    tbb::flow::function_node<int, int> fn_node(g, tbb::flow::unlimited, fn_body());
    tbb::flow::limiter_node<int> lim_node(g, _nparallel);
    tbb::flow::make_edge(tbb::flow::output_port<LIMITER_OUTPUT>(mf_node), lim_node);
    tbb::flow::make_edge(tbb::flow::output_port<DECREMENT_OUTPUT>(mf_node), lim_node.decrementer());
    tbb::flow::make_edge(lim_node, fn_node);
    tbb::flow::make_edge(fn_node, mf_node);

    mf_node.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE( (emit_count == receive_count), "counts do not match");
    CHECK_MESSAGE( (emit_sum == receive_sum), "sums do not match");

    // reset, test again
    g.reset();
    emit_count = 0;
    emit_sum = 0;
    receive_count = 0;
    receive_sum = 0;
    local_cnt = 0;
    mf_node.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE( (emit_count == receive_count), "counts do not match");
    CHECK_MESSAGE( (emit_sum == receive_sum), "sums do not match");
}


void
test_continue_msg_reception() {
    tbb::flow::graph g;
    tbb::flow::limiter_node<int> ln(g,2);
    tbb::flow::queue_node<int>   qn(g);
    tbb::flow::make_edge(ln, qn);
    ln.decrementer().try_put(tbb::flow::continue_msg());
    ln.try_put(42);
    g.wait_for_all();
    int outint;
    CHECK_MESSAGE( (qn.try_get(outint) && outint == 42), "initial put to decrement stops node");
}


//
// This test ascertains that if a message is not successfully put
// to a successor, the message is not dropped but released.
//

void test_reserve_release_messages() {
    using namespace tbb::flow;
    graph g;

    //making two queue_nodes: one broadcast_node and one limiter_node
    queue_node<int> input_queue(g);
    queue_node<int> output_queue(g);
    broadcast_node<int> broad(g);
    limiter_node<int, int> limit(g,2); //threshold of 2

    //edges
    make_edge(input_queue, limit);
    make_edge(limit, output_queue);
    make_edge(broad,limit.decrementer());

    int list[4] = {19, 33, 72, 98}; //list to be put to the input queue

    input_queue.try_put(list[0]); // succeeds
    input_queue.try_put(list[1]); // succeeds
    input_queue.try_put(list[2]); // fails, stored in upstream buffer
    g.wait_for_all();

    remove_edge(limit, output_queue); //remove successor

    //sending message to the decrement port of the limiter
    broad.try_put(1); //failed message retrieved.
    g.wait_for_all();

    tbb::flow::make_edge(limit, output_queue); //putting the successor back

    broad.try_put(1);  //drop the count

    input_queue.try_put(list[3]);  //success
    g.wait_for_all();

    int var=0;

    for (int i=0; i<4; i++) {
        output_queue.try_get(var);
        CHECK_MESSAGE( (var==list[i]), "some data dropped, input does not match output");
        g.wait_for_all();
    }
}

void test_decrementer() {
    const int threshold = 5;
    tbb::flow::graph g;
    tbb::flow::limiter_node<int, int> limit(g, threshold);
    tbb::flow::queue_node<int> queue(g);
    make_edge(limit, queue);
    int m = 0;
    CHECK_MESSAGE( ( limit.try_put( m++ )), "Newly constructed limiter node does not accept message." );
    CHECK_MESSAGE( limit.decrementer().try_put( -threshold ), // close limiter's gate
                   "Limiter node decrementer's port does not accept message." );
    CHECK_MESSAGE( ( !limit.try_put( m++ )), "Closed limiter node's accepts message." );
    CHECK_MESSAGE( limit.decrementer().try_put( threshold + 5 ),  // open limiter's gate
                   "Limiter node decrementer's port does not accept message." );
    for( int i = 0; i < threshold; ++i )
        CHECK_MESSAGE( ( limit.try_put( m++ )), "Limiter node does not accept message while open." );
    CHECK_MESSAGE( ( !limit.try_put( m )), "Limiter node's gate is not closed." );
    g.wait_for_all();
    int expected[] = {0, 2, 3, 4, 5, 6};
    int actual = -1; m = 0;
    while( queue.try_get(actual) )
        CHECK_MESSAGE( actual == expected[m++], "" );
    CHECK_MESSAGE( ( sizeof(expected) / sizeof(expected[0]) == m), "Not all messages have been processed." );
    g.wait_for_all();

    const size_t threshold2 = size_t(-1);
    tbb::flow::limiter_node<int, long long> limit2(g, threshold2);
    make_edge(limit2, queue);
    CHECK_MESSAGE( ( limit2.try_put( 1 )), "Newly constructed limiter node does not accept message." );
    long long decrement_value = (long long)( size_t(-1)/2 );
    CHECK_MESSAGE( limit2.decrementer().try_put( -decrement_value ),
                   "Limiter node decrementer's port does not accept message" );
    CHECK_MESSAGE( ( limit2.try_put( 2 )), "Limiter's gate should not be closed yet." );
    CHECK_MESSAGE( limit2.decrementer().try_put( -decrement_value ),
                   "Limiter node decrementer's port does not accept message" );
    CHECK_MESSAGE( ( !limit2.try_put( 3 )), "Overflow happened for internal counter." );
    int expected2[] = {1, 2};
    actual = -1; m = 0;
    while( queue.try_get(actual) )
        CHECK_MESSAGE( actual == expected2[m++], "" );
    CHECK_MESSAGE( ( sizeof(expected2) / sizeof(expected2[0]) == m), "Not all messages have been processed." );
    g.wait_for_all();

    const size_t threshold3 = 10;
    tbb::flow::limiter_node<int, long long> limit3(g, threshold3);
    make_edge(limit3, queue);
    long long decrement_value3 = 3;
    CHECK_MESSAGE( limit3.decrementer().try_put( -decrement_value3 ),
                   "Limiter node decrementer's port does not accept message" );

    m = 0;
    while( limit3.try_put( m ) ){ m++; };
    CHECK_MESSAGE( m == threshold3 - decrement_value3, "Not all messages have been accepted." );

    actual = -1; m = 0;
    while( queue.try_get(actual) ){
        CHECK_MESSAGE( actual == m++, "Not all messages have been processed." );
    }

    g.wait_for_all();
    CHECK_MESSAGE( m == threshold3 - decrement_value3, "Not all messages have been processed." );
}

void test_try_put_without_successors() {
    tbb::flow::graph g;
    int try_put_num{3};
    tbb::flow::buffer_node<int> bn(g);
    tbb::flow::limiter_node<int> ln(g, try_put_num);

    tbb::flow::make_edge(bn, ln);

    int i = 1;
    for (; i <= try_put_num; i++)
        bn.try_put(i);

    std::atomic<int> counter{0};
    tbb::flow::function_node<int, int> fn(g, tbb::flow::unlimited,
        [&](int input) {
            counter += input;
            return int{};
        }
    );

    tbb::flow::make_edge(ln, fn);

    g.wait_for_all();
    CHECK((counter == i * try_put_num / 2));

    // Check the lost message
    tbb::flow::remove_edge(bn, ln);
    ln.decrementer().try_put(tbb::flow::continue_msg());
    bn.try_put(try_put_num + 1);
    g.wait_for_all();
    CHECK((counter == i * try_put_num / 2));

}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows= { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t()};

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::limiter_node<msg_t, msg_t>>(messages_for_follows, 1000);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::limiter_node<msg_t, msg_t>>(messages_for_precedes, 1000);

}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;
    broadcast_node<int> br(g);
    limiter_node<int> l0(g, 100);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    limiter_node l1(follows(br), 100);
    static_assert(std::is_same_v<decltype(l1), limiter_node<int>>);

    limiter_node l2(precedes(br), 100);
    static_assert(std::is_same_v<decltype(l2), limiter_node<int>>);
#endif

    limiter_node l3(l0);
    static_assert(std::is_same_v<decltype(l3), limiter_node<int>>);
}
#endif

void test_decrement_while_try_put_task() {
    constexpr int threshold = 50000;

    tbb::flow::graph graph{};
    std::atomic<int> processed;
    tbb::flow::input_node<int> input{ graph, [&](tbb::flow_control & fc) -> int {
        static int i = {};
        if (i++ >= threshold) fc.stop();
        return i;
    }};
    tbb::flow::limiter_node<int, int> blockingNode{ graph, 1 };
    tbb::flow::multifunction_node<int, std::tuple<int>> processing{ graph, tbb::flow::serial,
        [&](const int & value, typename decltype(processing)::output_ports_type & out) {
            if (value != threshold)
                std::get<0>(out).try_put(1);
            processed.store(value);
        }};

    tbb::flow::make_edge(input, blockingNode);
    tbb::flow::make_edge(blockingNode, processing);
    tbb::flow::make_edge(processing, blockingNode.decrementer());

    input.activate();

    graph.wait_for_all();
    CHECK_MESSAGE(processed.load() == threshold, "decrementer terminate flow graph work");
}


//! Test puts on limiter_node with decrements and varying parallelism levels
//! \brief \ref error_guessing
TEST_CASE("Serial and parallel tests") {
    for (unsigned i = 1; i <= 2 * utils::MaxThread; ++i) {
        tbb::task_arena arena(i);
        arena.execute(
            [i]() {
                test_serial<int>();
                test_parallel<int>(i);
            }
        );
    }
}

//! Test initial put of continue_msg on decrementer port does not stop message flow
//! \brief \ref error_guessing
TEST_CASE("Test continue_msg reception") {
    test_continue_msg_reception();
}

//! Test put message on decrementer port does not stop message flow
//! \brief \ref error_guessing
TEST_CASE("Test try_put to decrementer while try_put to limiter_node") {
    test_decrement_while_try_put_task();
}

//! Test multifunction_node connected to limiter_node
//! \brief \ref error_guessing
TEST_CASE("Multifunction connected to limiter") {
    test_multifunction_to_limiter(30,3);
    test_multifunction_to_limiter(300,13);
    test_multifunction_to_limiter(3000,1);
}

//! Test message release if successor doesn't accept
//! \brief \ref requirement
TEST_CASE("Message is released if successor does not accept") {
    test_reserve_release_messages();
}

//! Test decrementer
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Decrementer") {
    test_decrementer();
}

//! Test try_put() without successor
//! \brief \ref error_guessing
TEST_CASE("Test try_put() without successors") {
    test_try_put_without_successors();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE( "Support for follows and precedes API" ) {
    test_follows_and_precedes_api();
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE( "Deduction guides" ) {
    test_deduction_guides();
}
#endif

struct TestLargeStruct {
    char bytes[512]{ 0 };
};

//! Test correct node deallocation while using small_object_pool.
//! (see https://github.com/oneapi-src/oneTBB/issues/639)
//! \brief \ref error_guessing
TEST_CASE("Test correct node deallocation while using small_object_pool") {
    tbb::flow::graph graph;
    tbb::flow::queue_node<TestLargeStruct> input_node( graph );
    tbb::flow::function_node<TestLargeStruct> func( graph, tbb::flow::serial,
        [](const TestLargeStruct& input) { return input; } );

    tbb::flow::make_edge( input_node, func );
    CHECK( input_node.try_put( TestLargeStruct{} ) );
    graph.wait_for_all();

    tbb::task_scheduler_handle handle{ tbb::attach{} };
    tbb::finalize( handle, std::nothrow );
}
