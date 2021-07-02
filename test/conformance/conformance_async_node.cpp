/*
    Copyright (c) 2020-2021 Intel Corporation

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

#include "common/test.h"

#include "common/utils.h"
#include "common/graph_utils.h"

#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/global_control.h"

#include "conformance_flowgraph.h"

//! \file conformance_async_node.cpp
//! \brief Test for [flow_graph.async_node] specification

/*
TODO: implement missing conformance tests for async_node:
  - [ ] Write `test_forwarding()'.
  - [ ] Improve test of the node's copy-constructor.
  - [ ] Write `test_priority'.
  - [ ] Rename `test_discarding' to `test_buffering'.
  - [ ] Write inheritance test.
  - [ ] Constructor with explicitly passed Policy parameter.
  - [ ] Concurrency testing of the node: make a loop over possible concurrency levels. It is
    important to test at least on five values: 1, oneapi::tbb::flow::serial, `max_allowed_parallelism'
    obtained from `oneapi::tbb::global_control', `oneapi::tbb::flow::unlimited', and, if `max allowed
    parallelism' is > 2, use something in the middle of the [1, max_allowed_parallelism]
    interval. Use `utils::ExactConcurrencyLevel' entity (extending it if necessary).
  - [ ] Write `test_rejecting', where avoid dependency on OS scheduling of the threads; add check
    that `try_put()' returns `false'
  - [ ] The `copy_body' function copies altered body (e.g. after successful `try_put()' call).
  - [ ] The copy constructor and copy assignment are called for the node's input and output types.
  - [ ] Add CTAD test.
*/

template<typename I, typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, async_node<I, O>>::value), "async_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<I>, async_node<I, O>>::value), "async_node should be derived from receiver<Input>");
    CHECK_MESSAGE( (std::is_base_of<sender<O>, async_node<I, O>>::value), "async_node should be derived from sender<Output>");
}

template< typename OutputType >
struct as_inc_functor {
    std::thread my_thread;

    std::atomic<size_t>& local_execute_count;

    as_inc_functor(std::atomic<size_t>& execute_count ) :
        local_execute_count (execute_count)
    {  }

    as_inc_functor( const as_inc_functor &f ) : local_execute_count(f.local_execute_count) { }
    void operator=(const as_inc_functor &f) { local_execute_count = size_t(f.local_execute_count); }

    void operator()( int num , oneapi::tbb::flow::async_node<int, int>::gateway_type& g) {
        ++local_execute_count;
        g.try_put(num);
        //    my_thread = std::thread([&](){
        //                                g.try_put(num);
        //                            });
    }

};

void test_async_body(){
    oneapi::tbb::flow::graph g;

    std::atomic<size_t> local_count(0);
    as_inc_functor<int> fun(local_count);

    oneapi::tbb::flow::async_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun);

    const size_t n = 10;
    for(size_t i = 0; i < n; ++i) {
        CHECK_MESSAGE((node1.try_put(1) == true), "try_put needs to return true");
    }

    //fun.my_thread.join();
    g.wait_for_all();

    CHECK_MESSAGE( (fun.local_execute_count.load() == n), "Body of the node needs to be executed N times");
}

void test_copy(){
    oneapi::tbb::flow::graph g;
    std::atomic<size_t> local_count(0);
    as_inc_functor<int> fun(local_count);

    oneapi::tbb::flow::async_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun);
    oneapi::tbb::flow::async_node<int, int> node2(node1);
}

void test_priority(){
    oneapi::tbb::flow::graph g;
    std::atomic<size_t> local_count(0);
    as_inc_functor<int> fun(local_count);

    oneapi::tbb::flow::async_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun, oneapi::tbb::flow::no_priority);
}

void test_discarding(){
    oneapi::tbb::flow::graph g;

    std::atomic<size_t> local_count(0);
    as_inc_functor<int> fun(local_count);

    oneapi::tbb::flow::async_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun);

    oneapi::tbb::flow::limiter_node< int > rejecter1( g,0);
    oneapi::tbb::flow::limiter_node< int > rejecter2( g,0);

    make_edge(node1, rejecter2);
    make_edge(node1, rejecter1);

    node1.try_put(1);

    int tmp = -1;
    CHECK_MESSAGE((node1.try_get(tmp) == false), "Value should be discarded after rejection");

    g.wait_for_all();
}

//! Test discarding property
//! \brief \ref requirement
TEST_CASE("async_node discarding") {
    test_discarding();

}

//! Test async_node priority interface
//! \brief \ref interface
TEST_CASE("async_node priority interface"){
    test_priority();
}

//! Test async_node copy
//! \brief \ref interface
TEST_CASE("async_node copy"){
    test_copy();
}

//! Test calling async body
//! \brief \ref interface \ref requirement
TEST_CASE("Test async_node body") {
    test_async_body();
}

//! Test async_node inheritance relations
//! \brief \ref interface
TEST_CASE("async_node superclasses"){
    test_inheritance<int, int>();
    test_inheritance<void*, float>();
}
