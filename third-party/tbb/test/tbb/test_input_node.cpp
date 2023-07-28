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

// have to expose the reset_node method to be able to reset a function_body

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/concepts_common.h"


//! \file test_input_node.cpp
//! \brief Test for [flow_graph.input_node] specification


using tbb::detail::d1::graph_task;
using tbb::detail::d1::SUCCESSFULLY_ENQUEUED;

const int N = 1000;

template< typename T >
class test_push_receiver : public tbb::flow::receiver<T>, utils::NoAssign {

    std::atomic<int> my_counters[N];
    tbb::flow::graph& my_graph;

public:

    test_push_receiver(tbb::flow::graph& g) : my_graph(g) {
        for (int i = 0; i < N; ++i )
            my_counters[i] = 0;
    }

    int get_count( int i ) {
        int v = my_counters[i];
        return v;
    }

    typedef typename tbb::flow::receiver<T>::predecessor_type predecessor_type;

    graph_task* try_put_task( const T &v ) override {
        int i = (int)v;
        ++my_counters[i];
        return const_cast<graph_task*>(SUCCESSFULLY_ENQUEUED);
    }

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }
};

template< typename T >
class my_input_body {

    unsigned my_count;
    int *ninvocations;

public:

    my_input_body() : ninvocations(nullptr) { my_count = 0; }
    my_input_body(int &_inv) : ninvocations(&_inv)  { my_count = 0; }

    T operator()( tbb::flow_control& fc ) {
        T v = (T)my_count++;
        if(ninvocations) ++(*ninvocations);
        if ( (int)v < N ){
            return v;
        }else{
            fc.stop();
            return T();
        }
    }

};

template< typename T >
class function_body {

    std::atomic<int> *my_counters;

public:

    function_body( std::atomic<int> *counters ) : my_counters(counters) {
        for (int i = 0; i < N; ++i )
            my_counters[i] = 0;
    }

    bool operator()( T v ) {
        ++my_counters[(int)v];
        return true;
    }

};

template< typename T >
void test_single_dest() {
    // push only
    tbb::flow::graph g;
    tbb::flow::input_node<T> src(g, my_input_body<T>() );
    test_push_receiver<T> dest(g);
    tbb::flow::make_edge( src, dest );
    src.activate();
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        CHECK_MESSAGE( dest.get_count(i) == 1, "" );
    }

    // push only
    std::atomic<int> counters3[N];
    tbb::flow::input_node<T> src3(g, my_input_body<T>() );
    src3.activate();

    function_body<T> b3( counters3 );
    tbb::flow::function_node<T,bool> dest3(g, tbb::flow::unlimited, b3 );
    tbb::flow::make_edge( src3, dest3 );
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 1, "" );
    }

    // push & pull
    tbb::flow::input_node<T> src2(g, my_input_body<T>() );
    src2.activate();
    std::atomic<int> counters2[N];

    function_body<T> b2( counters2 );
    tbb::flow::function_node<T,bool,tbb::flow::rejecting> dest2(g, tbb::flow::serial, b2 );
    tbb::flow::make_edge( src2, dest2 );
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters2[i];
        CHECK_MESSAGE( v == 1, "" );
    }

    // test copy constructor
    tbb::flow::input_node<T> src_copy(src);
    src_copy.activate();
    test_push_receiver<T> dest_c(g);
    CHECK_MESSAGE( src_copy.register_successor(dest_c), "" );
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        CHECK_MESSAGE( dest_c.get_count(i) == 1, "" );
    }
}

void test_reset() {
    //    input_node -> function_node
    tbb::flow::graph g;
    std::atomic<int> counters3[N];
    tbb::flow::input_node<int> src3(g, my_input_body<int>());
    src3.activate();
    tbb::flow::input_node<int> src_inactive(g, my_input_body<int>());
    function_body<int> b3( counters3 );
    tbb::flow::function_node<int,bool> dest3(g, tbb::flow::unlimited, b3);
    tbb::flow::make_edge( src3, dest3 );
    //    source_node already in active state.  Let the graph run,
    g.wait_for_all();
    //    check the array for each value.
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 1, "" );
        counters3[i] = 0;
    }

    g.reset(tbb::flow::rf_reset_bodies);  // <-- re-initializes the counts.
    // and spawns task to run input
    src3.activate();

    g.wait_for_all();
    //    check output queue again.  Should be the same contents.
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 1, "" );
        counters3[i] = 0;
    }
    g.reset();  // doesn't reset the input_node_body to initial state, but does spawn a task
                // to run the input_node.

    g.wait_for_all();
    // array should be all zero
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 0, "" );
    }

    remove_edge(src3, dest3);
    make_edge(src_inactive, dest3);

    // src_inactive doesn't run
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 0, "" );
    }

    // run graph
    src_inactive.activate();
    g.wait_for_all();
    // check output
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 1, "" );
        counters3[i] = 0;
    }
    g.reset(tbb::flow::rf_reset_bodies);  // <-- reinitializes the counts
    // src_inactive doesn't run
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 0, "" );
    }

    // start it up
    src_inactive.activate();
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 1, "" );
        counters3[i] = 0;
    }
    g.reset();  // doesn't reset the input_node_body to initial state, and doesn't
                // spawn a task to run the input_node.

    g.wait_for_all();
    // array should be all zero
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 0, "" );
    }
    src_inactive.activate();
    // input_node_body is already in final state, so input_node will not forward a message.
    g.wait_for_all();
    for (int i = 0; i < N; ++i ) {
        int v = counters3[i];
        CHECK_MESSAGE( v == 0, "" );
    }
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
void test_follows_and_precedes_api() {
    using namespace tbb::flow;

    graph g;

    std::array<buffer_node<bool>, 3> successors {{
                                                  buffer_node<bool>(g),
                                                  buffer_node<bool>(g),
                                                  buffer_node<bool>(g)
        }};

    bool do_try_put = true;
    input_node<bool> src(
        precedes(successors[0], successors[1], successors[2]),
        [&](tbb::flow_control& fc) -> bool {
            if(!do_try_put)
                fc.stop();
            do_try_put = !do_try_put;
            return true;
        }
    );

    src.activate();
    g.wait_for_all();

    bool storage;
    for(auto& successor: successors) {
        CHECK_MESSAGE((successor.try_get(storage) && !successor.try_get(storage)),
                      "Not exact edge quantity was made");
    }
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

//! Test push, push-pull behavior and copy constructor
//! \brief \ref error_guessing \ref requirement
TEST_CASE("Single destination tests"){
    for ( unsigned int p = utils::MinThread; p < utils::MaxThread; ++p ) {
        tbb::task_arena arena(p);
        arena.execute(
            [&]() {
                test_single_dest<int>();
                test_single_dest<float>();
            }
        );
	}
}

//! Test reset variants
//! \brief \ref error_guessing
TEST_CASE("Reset test"){
    test_reset();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Follows and precedes API"){
    test_follows_and_precedes_api();
}
#endif

//! Test try_get before activation
//! \brief \ref error_guessing
TEST_CASE("try_get before activation"){
    tbb::flow::graph g;
    tbb::flow::input_node<int> in(g, [&](tbb::flow_control& fc) { fc.stop(); return 0;});

    int tmp = -1;
    CHECK_MESSAGE((in.try_get(tmp) == false), "try_get before activation should not succeed");
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for input_node output") {
    struct Object : test_concepts::Copyable, test_concepts::CopyAssignable {};

    static_assert(utils::well_formed_instantiation<tbb::flow::input_node, Object>);
    static_assert(utils::well_formed_instantiation<tbb::flow::input_node, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::input_node, test_concepts::NonCopyable>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::input_node, test_concepts::NonCopyAssignable>);
}

template <typename Output, typename Body>
concept can_call_input_node_ctor = requires( tbb::flow::graph& graph, Body body, tbb::flow::buffer_node<int> f ) {
    tbb::flow::input_node<Output>(graph, body);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::input_node<Output>(tbb::flow::precedes(f), body);
#endif
};

//! \brief \ref error_guessing
TEST_CASE("constraints for input_node body") {
    using output_type = int;
    using namespace test_concepts::input_node_body;

    static_assert(can_call_input_node_ctor<output_type, Correct<output_type>>);
    static_assert(!can_call_input_node_ctor<output_type, NonCopyable<output_type>>);
    static_assert(!can_call_input_node_ctor<output_type, NonDestructible<output_type>>);
    static_assert(!can_call_input_node_ctor<output_type, NoOperatorRoundBrackets<output_type>>);
    static_assert(!can_call_input_node_ctor<output_type, WrongInputOperatorRoundBrackets<output_type>>);
    static_assert(!can_call_input_node_ctor<output_type, WrongReturnOperatorRoundBrackets<output_type>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
