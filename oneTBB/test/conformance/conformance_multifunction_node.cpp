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

//! \file conformance_multifunction_node.cpp
//! \brief Test for [flow_graph.function_node] specification

/*
TODO: implement missing conformance tests for multifunction_node:
  - [ ] Implement test_forwarding that checks messages are broadcast to all the successors connected
    to the output port the message is being sent to. And check that the value passed is the
    actual one received.
  - [ ] Explicit test for copy constructor of the node.
  - [ ] Constructor with explicitly passed Policy parameter: `template<typename Body>
    multifunction_node( graph &g, size_t concurrency, Body body, Policy(), node_priority_t priority = no_priority )'.
  - [ ] Concurrency testing of the node: make a loop over possible concurrency levels. It is
    important to test at least on five values: 1, oneapi::tbb::flow::serial, `max_allowed_parallelism'
    obtained from `oneapi::tbb::global_control', `oneapi::tbb::flow::unlimited', and, if `max allowed
    parallelism' is > 2, use something in the middle of the [1, max_allowed_parallelism]
    interval. Use `utils::ExactConcurrencyLevel' entity (extending it if necessary).
  - [ ] make `test_rejecting' deterministic, i.e. avoid dependency on OS scheduling of the threads;
    add check that `try_put()' returns `false'
  - [ ] The `copy_body' function copies altered body (e.g. after successful `try_put()' call).
  - [ ] `output_ports_type' is defined and accessible by the user.
  - [ ] Explicit test on `mfn::output_ports()' method.
  - [ ] The copy constructor and copy assignment are called for the node's input and output types.
  - [ ] Add CTAD test.
*/

template< typename OutputType >
struct mf_functor {

    std::atomic<std::size_t>& local_execute_count;

    mf_functor(std::atomic<std::size_t>& execute_count ) :
        local_execute_count (execute_count)
    {  }

    mf_functor( const mf_functor &f ) : local_execute_count(f.local_execute_count) { }
    void operator=(const mf_functor &f) { local_execute_count = std::size_t(f.local_execute_count); }

    void operator()( const int& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
       ++local_execute_count;
       std::get<0>(op).try_put(argument);
    }

};

template<typename I, typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, multifunction_node<I, O>>::value), "multifunction_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<I>, multifunction_node<I, O>>::value), "multifunction_node should be derived from receiver<Input>");
}

void test_multifunc_body(){
    oneapi::tbb::flow::graph g;
    std::atomic<size_t> local_count(0);
    mf_functor<std::tuple<int>> fun(local_count);

    oneapi::tbb::flow::multifunction_node<int, std::tuple<int>, oneapi::tbb::flow::rejecting> node1(g, oneapi::tbb::flow::unlimited, fun);

    const size_t n = 10;
    for(size_t i = 0; i < n; ++i) {
        CHECK_MESSAGE((node1.try_put(1) == true), "try_put needs to return true");
    }
    g.wait_for_all();

    CHECK_MESSAGE( (local_count == n), "Body of the node needs to be executed N times");
}

template<typename I, typename O>
struct CopyCounterBody{
    size_t copy_count;

    CopyCounterBody():
        copy_count(0) {}

    CopyCounterBody(const CopyCounterBody<I, O>& other):
        copy_count(other.copy_count + 1) {}

    CopyCounterBody& operator=(const CopyCounterBody<I, O>& other)
    { copy_count = other.copy_count + 1; return *this;}

    void operator()( const I& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
       std::get<0>(op).try_put(argument);
    }
};

void test_copies(){
     using namespace oneapi::tbb::flow;

     CopyCounterBody<int, std::tuple<int>> b;

     graph g;
     multifunction_node<int, std::tuple<int>> fn(g, unlimited, b);

     CopyCounterBody<int, std::tuple<int>> b2 = copy_body<CopyCounterBody<int, std::tuple<int>>,
                                                          multifunction_node<int, std::tuple<int>>>(fn);

     CHECK_MESSAGE( (b.copy_count + 2 <= b2.copy_count), "copy_body and constructor should copy bodies");
}

template< typename OutputType >
struct id_functor {
    void operator()( const int& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
       std::get<0>(op).try_put(argument);
    }
};

void test_forwarding(){
    oneapi::tbb::flow::graph g;
    id_functor<int> fun;

    oneapi::tbb::flow::multifunction_node<int, std::tuple<int>> node1(g, oneapi::tbb::flow::unlimited, fun);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node must receive one message.");
}

void test_rejecting_buffering(){
    oneapi::tbb::flow::graph g;
    id_functor<int> fun;

    oneapi::tbb::flow::multifunction_node<int, std::tuple<int>, oneapi::tbb::flow::rejecting> node(g, oneapi::tbb::flow::unlimited, fun);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);
    node.try_put(1);

    int tmp = -1;
    CHECK_MESSAGE( (std::get<0>(node.output_ports()).try_get(tmp) == false), "try_get after rejection should not succeed");
    CHECK_MESSAGE( (tmp == -1), "try_get after rejection should alter passed value");
    g.wait_for_all();
}

void test_policy_ctors(){
    using namespace oneapi::tbb::flow;
    graph g;

    id_functor<int> fun;

    multifunction_node<int, std::tuple<int>, lightweight> lw_node(g, oneapi::tbb::flow::serial, fun);
    multifunction_node<int, std::tuple<int>, queueing_lightweight> qlw_node(g, oneapi::tbb::flow::serial, fun);
    multifunction_node<int, std::tuple<int>, rejecting_lightweight> rlw_node(g, oneapi::tbb::flow::serial, fun);

}

std::atomic<size_t> my_concurrency;
std::atomic<size_t> my_max_concurrency;

struct concurrency_functor {
    void operator()( const int& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
        ++my_concurrency;

        size_t old_value = my_max_concurrency;
        while(my_max_concurrency < my_concurrency &&
              !my_max_concurrency.compare_exchange_weak(old_value, my_concurrency))
            ;

        size_t ms = 1000;
        std::chrono::milliseconds sleep_time( ms );
        std::this_thread::sleep_for( sleep_time );

        --my_concurrency;
        std::get<0>(op).try_put(argument);
    }

};

void test_node_concurrency(){
    my_concurrency = 0;
    my_max_concurrency = 0;

    oneapi::tbb::flow::graph g;

    concurrency_functor counter;
    oneapi::tbb::flow::multifunction_node <int, std::tuple<int>> fnode(g, oneapi::tbb::flow::serial, counter);

    test_push_receiver<int> sink(g);

    make_edge(std::get<0>(fnode.output_ports()), sink);

    for(int i = 0; i < 10; ++i){
        fnode.try_put(i);
    }

    g.wait_for_all();
    CHECK_MESSAGE( ( my_max_concurrency.load() == 1), "Measured parallelism over limit");
}


void test_priority(){
    size_t concurrency_limit = 1;
    oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_limit);

    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::continue_node<int> source(g,
                                         [](oneapi::tbb::flow::continue_msg){ return 1;});
    source.try_put(oneapi::tbb::flow::continue_msg());

    first_functor<int>::first_id = -1;
    first_functor<int> low_functor(1);
    first_functor<int> high_functor(2);

    oneapi::tbb::flow::multifunction_node<int, std::tuple<int>> high(g, oneapi::tbb::flow::unlimited, high_functor, oneapi::tbb::flow::node_priority_t(1));
    oneapi::tbb::flow::multifunction_node<int, std::tuple<int>> low(g, oneapi::tbb::flow::unlimited, low_functor);

    make_edge(source, low);
    make_edge(source, high);

    g.wait_for_all();

    CHECK_MESSAGE( (first_functor<int>::first_id == 2), "High priority node should execute first");
}

void test_rejecting(){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::multifunction_node <int, std::tuple<int>, oneapi::tbb::flow::rejecting> fnode(g, oneapi::tbb::flow::serial,
                                                                    [&](const int& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ){
                                                                        size_t ms = 50;
                                                                        std::chrono::milliseconds sleep_time( ms );
                                                                        std::this_thread::sleep_for( sleep_time );
                                                                        std::get<0>(op).try_put(argument);
                                                                    });

    test_push_receiver<int> sink(g);

    make_edge(std::get<0>(fnode.output_ports()), sink);

    for(int i = 0; i < 10; ++i){
        fnode.try_put(i);
    }

    g.wait_for_all();
    CHECK_MESSAGE( (get_count(sink) == 1), "Messages should be rejected while the first is being processed");
}

//! Test multifunction_node with rejecting policy
//! \brief \ref interface
TEST_CASE("multifunction_node with rejecting policy"){
    test_rejecting();
}

//! Test priorities
//! \brief \ref interface
TEST_CASE("multifunction_node priority"){
    test_priority();
}

//! Test concurrency
//! \brief \ref interface
TEST_CASE("multifunction_node concurrency"){
    test_node_concurrency();
}

//! Test constructors
//! \brief \ref interface
TEST_CASE("multifunction_node constructors"){
    test_policy_ctors();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("multifunction_node buffering"){
    test_rejecting_buffering();
}

//! Test function_node broadcasting
//! \brief \ref requirement
TEST_CASE("multifunction_node broadcast"){
    test_forwarding();
}

//! Test body copying and copy_body logic
//! \brief \ref interface
TEST_CASE("multifunction_node constructors"){
    test_copies();
}

//! Test calling function body
//! \brief \ref interface \ref requirement
TEST_CASE("multifunction_node body") {
    test_multifunc_body();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("multifunction_node superclasses"){
    test_inheritance<int, std::tuple<int>>();
    test_inheritance<void*, std::tuple<float>>();
}
