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

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/spin_barrier.h"


//! \file test_flow_graph.cpp
//! \brief Test for [flow_graph.continue_msg flow_graph.graph_node flow_graph.input_port flow_graph.output_port flow_graph.join_node flow_graph.split_node flow_graph.limiter_node flow_graph.write_once_node flow_graph.overwrite_node flow_graph.make_edge flow_graph.graph flow_graph.buffer_node flow_graph.function_node flow_graph.multifunction_node flow_graph.continue_node flow_graph.input_node] specification

const int T = 4;
const int W = 4;

struct decrement_wait : utils::NoAssign {

    tbb::flow::graph * const my_graph;
    bool * const my_done_flag;

    decrement_wait( tbb::flow::graph &h, bool *done_flag ) : my_graph(&h), my_done_flag(done_flag) {}

    void operator()(int i) const {
        utils::Sleep(10 * i);

        my_done_flag[i] = true;
        my_graph->release_wait();
    }
};

static void test_wait_count() {
    tbb::flow::graph h;
    for (int i = 0; i < T; ++i ) {
        bool done_flag[W];
        for (int j = 0; j < W; ++j ) {
            for ( int w = 0; w < W; ++w ) done_flag[w] = false;
            for ( int w = 0; w < j; ++w ) h.reserve_wait();

            utils::NativeParallelFor( j, decrement_wait(h, done_flag) );
            h.wait_for_all();
            for ( int w = 0; w < W; ++w ) {
                if ( w < j ) CHECK_MESSAGE( done_flag[w] == true, "" );
                else CHECK_MESSAGE( done_flag[w] == false, "" );
            }
        }
    }
}

// Encapsulate object we want to store in vector (because contained type must have
// copy constructor and assignment operator
class my_int_buffer {
    tbb::flow::buffer_node<int> *b;
    tbb::flow::graph& my_graph;
public:
    my_int_buffer(tbb::flow::graph &g) : my_graph(g) { b = new tbb::flow::buffer_node<int>(my_graph); }
    my_int_buffer(const my_int_buffer& other) : my_graph(other.my_graph) {
        b = new tbb::flow::buffer_node<int>(my_graph);
    }
    ~my_int_buffer() { delete b; }
    my_int_buffer& operator=(const my_int_buffer& /*other*/) {
        return *this;
    }
};

// test the graph iterator, delete nodes from graph, test again
void test_iterator() {
    tbb::flow::graph g;
    my_int_buffer a_buffer(g);
    my_int_buffer b_buffer(g);
    my_int_buffer c_buffer(g);
    my_int_buffer *d_buffer = new my_int_buffer(g);
    my_int_buffer e_buffer(g);
    std::vector< my_int_buffer > my_buffer_vector(10, c_buffer);

    int count = 0;
    for (tbb::flow::graph::iterator it = g.begin(); it != g.end(); ++it) {
        count++;
    }
    CHECK_MESSAGE( (count==15), "error in iterator count");

    delete d_buffer;

    count = 0;
    for (tbb::flow::graph::iterator it = g.begin(); it != g.end(); ++it) {
        count++;
    }
    CHECK_MESSAGE( (count==14), "error in iterator count");

    my_buffer_vector.clear();

    count = 0;
    for (tbb::flow::graph::iterator it = g.begin(); it != g.end(); ++it) {
        count++;
    }
    CHECK_MESSAGE( (count==4), "error in iterator count");
}

class AddRemoveBody : utils::NoAssign {
    tbb::flow::graph& g;
    int nThreads;
    utils::SpinBarrier &barrier;
public:
    AddRemoveBody(int nthr, utils::SpinBarrier &barrier_, tbb::flow::graph& _g) :
        g(_g), nThreads(nthr), barrier(barrier_)
    {}
    void operator()(const int /*threadID*/) const {
        my_int_buffer b(g);
        {
            std::vector<my_int_buffer> my_buffer_vector(100, b);
            barrier.wait();  // wait until all nodes are created
            // now test that the proper number of nodes were created
            int count = 0;
            for (tbb::flow::graph::iterator it = g.begin(); it != g.end(); ++it) {
                count++;
            }
            CHECK_MESSAGE( (count==101*nThreads), "error in iterator count");
            barrier.wait();  // wait until all threads are done counting
        } // all nodes but for the initial node on this thread are deleted
        barrier.wait(); // wait until all threads have deleted all nodes in their vectors
        // now test that all the nodes were deleted except for the initial node
        int count = 0;
        for (tbb::flow::graph::iterator it = g.begin(); it != g.end(); ++it) {
            count++;
        }
        CHECK_MESSAGE( (count==nThreads), "error in iterator count");
        barrier.wait();  // wait until all threads are done counting
    } // initial node gets deleted
};

void test_parallel(int nThreads) {
    tbb::flow::graph g;
    utils::SpinBarrier barrier(nThreads);
    AddRemoveBody body(nThreads, barrier, g);
    NativeParallelFor(nThreads, body);
}

/*
 * Functors for graph arena spawn tests
 */

inline void check_arena(tbb::task_arena* midway_arena) {
    CHECK_MESSAGE(midway_arena->max_concurrency() == 2, "");
    CHECK_MESSAGE(tbb::this_task_arena::max_concurrency() == 1, "");
}

struct run_functor {
    tbb::task_arena* midway_arena;
    int return_value;
    run_functor(tbb::task_arena* a) : midway_arena(a), return_value(1) {}
    int operator()() {
        check_arena(midway_arena);
        return return_value;
    }
};

template < typename T >
struct function_body {
    tbb::task_arena* midway_arena;
    function_body(tbb::task_arena* a) : midway_arena(a) {}
    tbb::flow::continue_msg operator()(const T& /*arg*/) {
        check_arena(midway_arena);
        return tbb::flow::continue_msg();
    }
};

typedef tbb::flow::multifunction_node< int, std::tuple< int > > mf_node;

struct multifunction_body {
    tbb::task_arena* midway_arena;
    multifunction_body(tbb::task_arena* a) : midway_arena(a) {}
    void operator()(const int& /*arg*/, mf_node::output_ports_type& /*outports*/) {
        check_arena(midway_arena);
    }
};

struct input_body {
    tbb::task_arena* midway_arena;
    int counter;
    input_body(tbb::task_arena* a) : midway_arena(a), counter(0) {}
    int operator()(tbb::flow_control &fc) {
        check_arena(midway_arena);
        if (counter++ >= 1) {
            fc.stop();
        }
        return int();
    }
};

struct nodes_test_functor : utils::NoAssign {
    tbb::task_arena* midway_arena;
    tbb::flow::graph& my_graph;

    nodes_test_functor(tbb::task_arena* a, tbb::flow::graph& g) : midway_arena(a), my_graph(g) {}
    void operator()() const {

        // Define test nodes
        // Continue, function, source nodes
        tbb::flow::continue_node< tbb::flow::continue_msg > c_n(my_graph, function_body<tbb::flow::continue_msg>(midway_arena));
        tbb::flow::function_node< int > f_n(my_graph, tbb::flow::unlimited, function_body<int>(midway_arena));
        tbb::flow::input_node< int > s_n(my_graph, input_body(midway_arena));

        // Multifunction node
        mf_node m_n(my_graph, tbb::flow::unlimited, multifunction_body(midway_arena));

        // Join node
        tbb::flow::function_node< std::tuple< int, int > > join_f_n(
            my_graph, tbb::flow::unlimited, function_body< std::tuple< int, int > >(midway_arena)
        );
        tbb::flow::join_node< std::tuple< int, int > > j_n(my_graph);
        make_edge(j_n, join_f_n);

        // Split node
        tbb::flow::function_node< int > split_f_n1 = f_n;
        tbb::flow::function_node< int > split_f_n2 = f_n;
        tbb::flow::split_node< std::tuple< int, int > > sp_n(my_graph);
        make_edge(tbb::flow::output_port<0>(sp_n), split_f_n1);
        make_edge(tbb::flow::output_port<1>(sp_n), split_f_n2);

        // Overwrite node
        tbb::flow::function_node< int > ow_f_n = f_n;
        tbb::flow::overwrite_node< int > ow_n(my_graph);
        make_edge(ow_n, ow_f_n);

        // Write once node
        tbb::flow::function_node< int > w_f_n = f_n;
        tbb::flow::write_once_node< int > w_n(my_graph);
        make_edge(w_n, w_f_n);

        // Buffer node
        tbb::flow::function_node< int > buf_f_n = f_n;
        tbb::flow::buffer_node< int > buf_n(my_graph);
        make_edge(w_n, buf_f_n);

        // Limiter node
        tbb::flow::function_node< int > l_f_n = f_n;
        tbb::flow::limiter_node< int > l_n(my_graph, 1);
        make_edge(l_n, l_f_n);

        // Execute nodes
        c_n.try_put( tbb::flow::continue_msg() );
        f_n.try_put(1);
        m_n.try_put(1);
        s_n.activate();

        tbb::flow::input_port<0>(j_n).try_put(1);
        tbb::flow::input_port<1>(j_n).try_put(1);

        std::tuple< int, int > sp_tuple(1, 1);
        sp_n.try_put(sp_tuple);

        ow_n.try_put(1);
        w_n.try_put(1);
        buf_n.try_put(1);
        l_n.try_put(1);

        my_graph.wait_for_all();
    }
};

void test_graph_arena() {
    // There is only one thread for execution (external thread).
    // So, if graph's tasks get spawned in different arena
    // external thread won't be able to find them in its own arena.
    // In this case test should hang.
    tbb::task_arena arena(1);
	arena.execute(
        [&]() {
            tbb::flow::graph g;
            tbb::task_arena midway_arena;
            midway_arena.initialize(2);
            midway_arena.execute(nodes_test_functor(&midway_arena, g));

        }
	);
}

//! Test wait counts
//! \brief error_guessing
TEST_CASE("Test wait_count"){
    for(unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_wait_count();
            }
        );
	}
}

//! Test graph iterators
//! \brief interface
TEST_CASE("Test graph::iterator"){
    for(unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_iterator();
            }
        );
	}
}

//! Test parallel for body
//! \brief \ref error_guessing
TEST_CASE("Test parallel"){
    for(unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_parallel(p);
            }
        );
	}
}

//! Test separate arena isn't used
//! \brief \ref error_guessing
TEST_CASE("Test graph_arena"){
    test_graph_arena();
}

//! Graph iterator
//! \brief \ref error_guessing
TEST_CASE("graph iterator") {
    using namespace tbb::flow;

    graph g;
    
    auto past_end = g.end();
    ++past_end;

    continue_node<int> n(g, [](const continue_msg &){return 1;});

    size_t item_count = 0;

    for(auto it = g.cbegin(); it != g.cend(); it++)
        ++item_count;
    CHECK_MESSAGE((item_count == 1), "Should find 1 item");

    item_count = 0;
    auto jt(g.begin());
    for(; jt != g.end(); jt++)
        ++item_count;
    CHECK_MESSAGE((item_count == 1), "Should find 1 item");

    graph g2;
    continue_node<int> n2(g, [](const continue_msg &){return 1;});
    CHECK_MESSAGE((g.begin() != g2.begin()), "Different graphs should have different iterators");
}
