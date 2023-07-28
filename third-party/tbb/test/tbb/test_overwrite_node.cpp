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

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"


//! \file test_overwrite_node.cpp
//! \brief Test for [flow_graph.overwrite_node] specification


#define N 300
#define T 4
#define M 5

template< typename R >
void simple_read_write_tests() {
    tbb::flow::graph g;
    tbb::flow::overwrite_node<R> n(g);

    for ( int t = 0; t < T; ++t ) {
        R v0(N+1);
        std::vector< std::shared_ptr<harness_counting_receiver<R>> > r;
        for (size_t i = 0; i < M; ++i) {
            r.push_back( std::make_shared<harness_counting_receiver<R>>(g) );
        }

        CHECK_MESSAGE( n.is_valid() == false, "" );
        CHECK_MESSAGE( n.try_get( v0 ) == false, "" );
        if ( t % 2 ) {
            CHECK_MESSAGE( n.try_put( static_cast<R>(N) ), "" );
            CHECK_MESSAGE( n.is_valid() == true, "" );
            CHECK_MESSAGE( n.try_get( v0 ) == true, "" );
            CHECK_MESSAGE( v0 == R(N), "" );
        }

        for (int i = 0; i < M; ++i) {
            tbb::flow::make_edge( n, *r[i] );
        }

        for (int i = 0; i < N; ++i ) {
            R v1(static_cast<R>(i));
            CHECK_MESSAGE( n.try_put( v1 ), "" );
            CHECK_MESSAGE( n.is_valid() == true, "" );
            for (int j = 0; j < N; ++j ) {
                R v2(0);
                CHECK_MESSAGE( n.try_get( v2 ), "" );
                CHECK_MESSAGE( v1 == v2, "" );
            }
        }
        for (int i = 0; i < M; ++i) {
            size_t c = r[i]->my_count;
            CHECK_MESSAGE( int(c) == N+t%2, "" );
        }
        for (int i = 0; i < M; ++i) {
            tbb::flow::remove_edge( n, *r[i] );
        }
        CHECK_MESSAGE( n.try_put( R(0) ), "" );
        for (int i = 0; i < M; ++i) {
            size_t c = r[i]->my_count;
            CHECK_MESSAGE( int(c) == N+t%2, "" );
        }
        n.clear();
        CHECK_MESSAGE( n.is_valid() == false, "" );
        CHECK_MESSAGE( n.try_get( v0 ) == false, "" );
    }
}

template< typename R >
class native_body : utils::NoAssign {
    tbb::flow::overwrite_node<R> &my_node;

public:

    native_body( tbb::flow::overwrite_node<R> &n ) : my_node(n) {}

    void operator()( int i ) const {
        R v1(static_cast<R>(i));
        CHECK_MESSAGE( my_node.try_put( v1 ), "" );
        CHECK_MESSAGE( my_node.is_valid() == true, "" );
    }
};

template< typename R >
void parallel_read_write_tests() {
    tbb::flow::graph g;
    tbb::flow::overwrite_node<R> n(g);
    //Create a vector of identical nodes
    std::vector< tbb::flow::overwrite_node<R> > ow_vec(2, n);

    for (size_t node_idx=0; node_idx<ow_vec.size(); ++node_idx) {
        for ( int t = 0; t < T; ++t ) {
            std::vector< std::shared_ptr<harness_counting_receiver<R>> > r;
            for (size_t i = 0; i < M; ++i) {
                r.push_back( std::make_shared<harness_counting_receiver<R>>(g) );
            }

            for (int i = 0; i < M; ++i) {
                tbb::flow::make_edge( ow_vec[node_idx], *r[i] );
            }
            R v0;
            CHECK_MESSAGE( ow_vec[node_idx].is_valid() == false, "" );
            CHECK_MESSAGE( ow_vec[node_idx].try_get( v0 ) == false, "" );

#if TBB_TEST_LOW_WORKLOAD
            const int nthreads = 30;
#else
            const int nthreads = N;
#endif
            utils::NativeParallelFor( nthreads, native_body<R>( ow_vec[node_idx] ) );

            for (int i = 0; i < M; ++i) {
                size_t c = r[i]->my_count;
                CHECK_MESSAGE( int(c) == nthreads, "" );
            }
            for (int i = 0; i < M; ++i) {
                tbb::flow::remove_edge( ow_vec[node_idx], *r[i] );
            }
            CHECK_MESSAGE( ow_vec[node_idx].try_put( R(0) ), "" );
            for (int i = 0; i < M; ++i) {
                size_t c = r[i]->my_count;
                CHECK_MESSAGE( int(c) == nthreads, "" );
            }
            ow_vec[node_idx].clear();
            CHECK_MESSAGE( ow_vec[node_idx].is_valid() == false, "" );
            CHECK_MESSAGE( ow_vec[node_idx].try_get( v0 ) == false, "" );
        }
    }
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t()};

    follows_and_precedes_testing::test_follows<msg_t, tbb::flow::overwrite_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes<msg_t, tbb::flow::overwrite_node<msg_t>>(messages_for_precedes);
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;
    broadcast_node<int> b1(g);
    overwrite_node<int> o0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    overwrite_node o1(follows(b1));
    static_assert(std::is_same_v<decltype(o1), overwrite_node<int>>);

    overwrite_node o2(precedes(b1));
    static_assert(std::is_same_v<decltype(o2), overwrite_node<int>>);
#endif

    overwrite_node o3(o0);
    static_assert(std::is_same_v<decltype(o3), overwrite_node<int>>);
}
#endif

//! Test read-write properties
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Read-write"){
    simple_read_write_tests<int>();
    simple_read_write_tests<float>();
}

//! Read-write and ParallelFor tests under limited parallelism
//! \brief \ref error_guessing
TEST_CASE("Limited parallelism"){
    for( unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                parallel_read_write_tests<int>();
                parallel_read_write_tests<float>();
                test_reserving_nodes<tbb::flow::overwrite_node, size_t>();
            }
        );
	}
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Follows and precedes API"){
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

//! Test try_release
//! \brief \ref error_guessing
TEST_CASE("try_release"){
    tbb::flow::graph g;

    tbb::flow::overwrite_node<int> on(g);

    CHECK_MESSAGE ((on.try_release()== true), "try_release should return true");
}

//! Test for cancel register_predecessor_task
//! \brief \ref error_guessing
TEST_CASE("Cancel register_predecessor_task") {
    tbb::flow::graph g;
    // Cancel graph context for preventing tasks execution and
    // calling cancel method of spawned tasks
    g.cancel();

    // To spawn register_predecessor_task internal buffer of overwrite_node
    // should be valid and successor should failed during putting an item to it
    oneapi::tbb::flow::overwrite_node<size_t> node(g);
    // Reserving join_node always fails during putting an item to it
    tbb::flow::join_node<std::tuple<size_t>, tbb::flow::reserving> j_node(g);

    // Make internal buffer of overwrite_node valid
    node.try_put(1);
    // Making an edge attempts pushing an item to join_node
    // that immediately fails and tries to reverse an edge into PULL state
    // by spawning register_predecessor_task, which will be cancelled
    // during execution
    tbb::flow::make_edge(node, tbb::flow::input_port<0>(j_node));

    // Wait for cancellation of spawned tasks
    g.wait_for_all();
}
