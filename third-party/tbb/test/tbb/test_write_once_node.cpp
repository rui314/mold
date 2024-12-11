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
#include "common/utils_assert.h"
#include "common/graph_utils.h"
#include "common/test_follows_and_precedes_api.h"

#define N 300
#define T 4
#define M 4


//! \file test_write_once_node.cpp
//! \brief Test for [flow_graph.write_once_node] specification


template< typename R >
void simple_read_write_tests() {
    tbb::flow::graph g;
    tbb::flow::write_once_node<R> n(g);

    for ( int t = 0; t < T; ++t ) {
        R v0(0);
        std::vector< std::shared_ptr<harness_counting_receiver<R>> > r;
        for (size_t i = 0; i < M; ++i) {
            r.push_back( std::make_shared<harness_counting_receiver<R>>(g) );
        }


        CHECK_MESSAGE( n.is_valid() == false, "" );
        CHECK_MESSAGE( n.try_get( v0 ) == false, "" );

        if ( t % 2 ) {
            CHECK_MESSAGE( n.try_put( static_cast<R>(N+1) ), "" );
            CHECK_MESSAGE( n.is_valid() == true, "" );
            CHECK_MESSAGE( n.try_get( v0 ) == true, "" );
            CHECK_MESSAGE( v0 == R(N+1), "" );
        }

        for (int i = 0; i < M; ++i) {
            tbb::flow::make_edge( n, *r[i] );
        }

        if ( t%2 ) {
            for (int i = 0; i < M; ++i) {
                size_t c = r[i]->my_count;
                CHECK_MESSAGE( int(c) == 1, "" );
            }
        }

        for (int i = 1; i <= N; ++i ) {
            R v1(static_cast<R>(i));

            bool result = n.try_put( v1 );
            if ( !(t%2) && i == 1 )
                CHECK_MESSAGE( result == true, "" );
            else
                CHECK_MESSAGE( result == false, "" );

            CHECK_MESSAGE( n.is_valid() == true, "" );

            for (int j = 0; j < N; ++j ) {
                R v2(0);
                CHECK_MESSAGE( n.try_get( v2 ), "" );
                if ( t%2 )
                    CHECK_MESSAGE( R(N+1) == v2, "" );
                else
                    CHECK_MESSAGE( R(1) == v2, "" );
            }
        }
        for (int i = 0; i < M; ++i) {
            size_t c = r[i]->my_count;
            CHECK_MESSAGE( int(c) == 1, "" );
        }
        for (int i = 0; i < M; ++i) {
            tbb::flow::remove_edge( n, *r[i] );
        }
        CHECK_MESSAGE( n.try_put( R(0) ) == false, "" );
        for (int i = 0; i < M; ++i) {
            size_t c = r[i]->my_count;
            CHECK_MESSAGE( int(c) == 1, "" );
        }
        n.clear();
        CHECK_MESSAGE( n.is_valid() == false, "" );
        CHECK_MESSAGE( n.try_get( v0 ) == false, "" );
    }
}

template< typename R >
class native_body : utils::NoAssign {
    tbb::flow::write_once_node<R> &my_node;

public:

    native_body( tbb::flow::write_once_node<R> &n ) : my_node(n) {}

    void operator()( int i ) const {
        R v1(static_cast<R>(i));
        CHECK_MESSAGE( my_node.try_put( v1 ) == false, "" );
        CHECK_MESSAGE( my_node.is_valid() == true, "" );
        CHECK_MESSAGE( my_node.try_get( v1 ) == true, "" );
        CHECK_MESSAGE( v1 == R(-1), "" );
    }
};

template< typename R >
void parallel_read_write_tests() {
    tbb::flow::graph g;
    tbb::flow::write_once_node<R> n(g);
    //Create a vector of identical nodes
    std::vector< tbb::flow::write_once_node<R> > wo_vec(2, n);

    for (size_t node_idx=0; node_idx<wo_vec.size(); ++node_idx) {
        for ( int t = 0; t < T; ++t ) {
            std::vector< std::shared_ptr<harness_counting_receiver<R>> > r;
            for (size_t i = 0; i < M; ++i) {
                r.push_back( std::make_shared<harness_counting_receiver<R>>(g) );
            }


            for (int i = 0; i < M; ++i) {
                tbb::flow::make_edge( wo_vec[node_idx], *r[i] );
            }
            R v0;
            CHECK_MESSAGE( wo_vec[node_idx].is_valid() == false, "" );
            CHECK_MESSAGE( wo_vec[node_idx].try_get( v0 ) == false, "" );

            CHECK_MESSAGE( wo_vec[node_idx].try_put( R(-1) ), "" );
#if TBB_TEST_LOW_WORKLOAD
            const int nthreads = 30;
#else
            const int nthreads = N;
#endif
            utils::NativeParallelFor( nthreads, native_body<R>( wo_vec[node_idx] ) );

            for (int i = 0; i < M; ++i) {
                size_t c = r[i]->my_count;
                CHECK_MESSAGE( int(c) == 1, "" );
            }
            for (int i = 0; i < M; ++i) {
                tbb::flow::remove_edge( wo_vec[node_idx], *r[i] );
            }
            CHECK_MESSAGE( wo_vec[node_idx].try_put( R(0) ) == false, "" );
            for (int i = 0; i < M; ++i) {
                size_t c = r[i]->my_count;
                CHECK_MESSAGE( int(c) == 1, "" );
            }
            wo_vec[node_idx].clear();
            CHECK_MESSAGE( wo_vec[node_idx].is_valid() == false, "" );
            CHECK_MESSAGE( wo_vec[node_idx].try_get( v0 ) == false, "" );
        }
    }
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
void test_follows_and_precedes_api() {
    using msg_t = tbb::flow::continue_msg;

    std::array<msg_t, 3> messages_for_follows= {msg_t(), msg_t(), msg_t()};
    std::vector<msg_t> messages_for_precedes = {msg_t()};

    follows_and_precedes_testing::test_follows<msg_t, tbb::flow::write_once_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_precedes<msg_t, tbb::flow::write_once_node<msg_t>>(messages_for_precedes);
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;
    broadcast_node<int> b1(g);
    write_once_node<int> wo0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    write_once_node wo1(follows(b1));
    static_assert(std::is_same_v<decltype(wo1), write_once_node<int>>);

    write_once_node wo2(precedes(b1));
    static_assert(std::is_same_v<decltype(wo2), write_once_node<int>>);
#endif

    write_once_node wo3(wo0);
    static_assert(std::is_same_v<decltype(wo3), write_once_node<int>>);
}
#endif

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
void test_try_put_and_wait() {
    int wait_message = 0;
    int occupy_concurrency_message = 1;
    int new_message = 2;

    // Test push
    {
        tbb::task_arena arena(1);

        std::vector<int> processed_items;

        arena.execute([&] {
            tbb::flow::graph g;

            tbb::flow::write_once_node<int> wo_buffer(g);
            tbb::flow::function_node<int, int> function(g, tbb::flow::serial,
                [&](int input) {
                    if (input == wait_message) {
                        wo_buffer.clear();
                        wo_buffer.try_put(new_message);
                    }
                    processed_items.emplace_back(input);
                    return 0;
                });

            tbb::flow::make_edge(wo_buffer, function);

            wo_buffer.try_put_and_wait(wait_message);

            std::size_t check_index = 0;
            CHECK_MESSAGE(processed_items.size() == 1, "Only the wait_message should be processed");
            CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Only the wait_message should be processed");

            g.wait_for_all();

            CHECK_MESSAGE(processed_items[check_index++] == new_message,
                          "only the new_message should be processed in wait_for_all");
            CHECK(check_index == processed_items.size());
        });
    }
    // Test pull
    {
        std::vector<int> processed_items;
        tbb::task_arena arena(1);

        arena.execute([&] {
            tbb::flow::graph g;

            tbb::flow::write_once_node<int> wo_buffer(g);
            tbb::flow::function_node<int, int, tbb::flow::rejecting> function(g, tbb::flow::serial,
                [&](int input) {
                    if (input == new_message || input == wait_message) {
                        wo_buffer.clear();
                    }

                    if (input == wait_message) {
                        wo_buffer.try_put(new_message);
                    }
                    processed_items.emplace_back(input);
                    return 0;
                });

            tbb::flow::make_edge(wo_buffer, function);

            function.try_put(occupy_concurrency_message);
            wo_buffer.try_put_and_wait(wait_message);

            std::size_t check_index = 0;
            CHECK_MESSAGE(processed_items.size() == 2, "unexpected message processing for try_put_and_wait");
            CHECK_MESSAGE(processed_items[check_index++] == occupy_concurrency_message,
                          "occupy_concurrency_message should be processed first");
            CHECK_MESSAGE(processed_items[check_index++] == wait_message,
                          "wait_message was not processed");

            g.wait_for_all();

            CHECK_MESSAGE(processed_items[check_index++] == new_message,
                          "only the new_message should be processed in wait_for_all");
            CHECK(check_index == processed_items.size());
        });
    }
    // Test reserve
    {
        std::vector<int> processed_items;
        tbb::task_arena arena(1);

        arena.execute([&] {
            tbb::flow::graph g;

            tbb::flow::write_once_node<int> wo_buffer(g);
            tbb::flow::limiter_node<int, int> limiter(g, 1);
            tbb::flow::function_node<int, int, tbb::flow::rejecting> function(g, tbb::flow::serial,
                [&](int input) {
                    if (input == new_message || input == wait_message) {
                        wo_buffer.clear();
                    }

                    if (input == wait_message) {
                        wo_buffer.try_put(new_message);
                    }
                    processed_items.emplace_back(input);
                    limiter.decrementer().try_put(1);
                    return 0;
                });

            tbb::flow::make_edge(wo_buffer, limiter);
            tbb::flow::make_edge(limiter, function);

            limiter.try_put(occupy_concurrency_message);
            wo_buffer.try_put_and_wait(wait_message);

            std::size_t check_index = 0;
            CHECK_MESSAGE(processed_items.size() == 2, "unexpected message processing for try_put_and_wait");
            CHECK_MESSAGE(processed_items[check_index++] == occupy_concurrency_message,
                          "occupy_concurrency_message should be processed first");
            CHECK_MESSAGE(processed_items[check_index++] == wait_message,
                          "wait_message was not processed");

            g.wait_for_all();

            CHECK_MESSAGE(processed_items[check_index++] == new_message,
                          "only the new_message should be processed in wait_for_all");
            CHECK(check_index == processed_items.size());
        });
    }
}
#endif

//! Test read-write properties
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Read-write tests"){
    simple_read_write_tests<int>();
    simple_read_write_tests<float>();
}

//! Test read-write properties under parallelism
//! \brief \ref requirement \ref error_guessing \ref stress
TEST_CASE("Parallel read-write tests"){
    for( unsigned int p=utils::MinThread; p<=utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                parallel_read_write_tests<int>();
                parallel_read_write_tests<float>();
                test_reserving_nodes<tbb::flow::write_once_node, size_t>();
            }
        );
	}
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test deprecated follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and precedes API"){
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
TEST_CASE("test write_once_node try_put_and_wait") {
    test_try_put_and_wait();
}
#endif
