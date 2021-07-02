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

//! \file conformance_function_node.cpp
//! \brief Test for [flow_graph.function_node] specification

/*
TODO: implement missing conformance tests for function_node:
  - [ ] Constructor with explicitly passed Policy parameter: `template<typename Body> function_node(
    graph &g, size_t concurrency, Body body, Policy(), node_priority_t, priority = no_priority )'
  - [ ] Explicit test for copy constructor of the node.
  - [ ] Rename test_broadcast to test_forwarding and check that the value passed is the actual one
    received.
  - [ ] Concurrency testing of the node: make a loop over possible concurrency levels. It is
    important to test at least on five values: 1, oneapi::tbb::flow::serial, `max_allowed_parallelism'
    obtained from `oneapi::tbb::global_control', `oneapi::tbb::flow::unlimited', and, if `max allowed
    parallelism' is > 2, use something in the middle of the [1, max_allowed_parallelism]
    interval. Use `utils::ExactConcurrencyLevel' entity (extending it if necessary).
  - [ ] make `test_rejecting' deterministic, i.e. avoid dependency on OS scheduling of the threads;
    add check that `try_put()' returns `false'
  - [ ] The copy constructor and copy assignment are called for the node's input and output types.
  - [ ] The `copy_body' function copies altered body (e.g. after successful `try_put()' call).
  - [ ] Extend CTAD test to check all node's constructors.
*/

std::atomic<size_t> my_concurrency;
std::atomic<size_t> my_max_concurrency;

template< typename OutputType >
struct concurrency_functor {
    OutputType operator()( int argument ) {
        ++my_concurrency;

        size_t old_value = my_max_concurrency;
        while(my_max_concurrency < my_concurrency &&
              !my_max_concurrency.compare_exchange_weak(old_value, my_concurrency))
            ;

        size_t ms = 1000;
        std::chrono::milliseconds sleep_time( ms );
        std::this_thread::sleep_for( sleep_time );

        --my_concurrency;
       return argument;
    }

};

void test_func_body(){
    oneapi::tbb::flow::graph g;
    inc_functor<int> fun;
    fun.execute_count = 0;

    oneapi::tbb::flow::function_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun);

    const size_t n = 10;
    for(size_t i = 0; i < n; ++i) {
        CHECK_MESSAGE((node1.try_put(1) == true), "try_put needs to return true");
    }
    g.wait_for_all();

    CHECK_MESSAGE( (fun.execute_count == n), "Body of the node needs to be executed N times");
}

void test_priority(){
    size_t concurrency_limit = 1;
    oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_limit);

    oneapi::tbb::flow::graph g;

    first_functor<int>::first_id.store(-1);
    first_functor<int> low_functor(1);
    first_functor<int> high_functor(2);

    oneapi::tbb::flow::continue_node<int> source(g, [&](oneapi::tbb::flow::continue_msg){return 1;} );

    oneapi::tbb::flow::function_node<int, int> high(g, oneapi::tbb::flow::unlimited, high_functor, oneapi::tbb::flow::node_priority_t(1));
    oneapi::tbb::flow::function_node<int, int> low(g, oneapi::tbb::flow::unlimited, low_functor);

    make_edge(source, low);
    make_edge(source, high);

    source.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE( (first_functor<int>::first_id == 2), "High priority node should execute first");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides(){
    using namespace oneapi::tbb::flow;
    graph g;

    auto body = [](const int&)->int { return 1; };
    function_node f1(g, unlimited, body);
    CHECK_MESSAGE((std::is_same_v<decltype(f1), function_node<int, int>>), "Function node type must be deducible from its body");
}
#endif

void test_broadcast(){
    oneapi::tbb::flow::graph g;
    passthru_body fun;

    oneapi::tbb::flow::function_node<int, int> node1(g, oneapi::tbb::flow::unlimited, fun);
    test_push_receiver<int> node2(g);
    test_push_receiver<int> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    node1.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node must receive one message.");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

template<typename Policy>
void test_buffering(){
    oneapi::tbb::flow::graph g;
    passthru_body fun;

    oneapi::tbb::flow::function_node<int, int, Policy> node(g, oneapi::tbb::flow::unlimited, fun);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(node, rejecter);
    node.try_put(1);

    int tmp = -1;
    CHECK_MESSAGE( (node.try_get(tmp) == false), "try_get after rejection should not succeed");
    CHECK_MESSAGE( (tmp == -1), "try_get after rejection should not alter passed value");
    g.wait_for_all();
}

void test_node_concurrency(){
    my_concurrency = 0;
    my_max_concurrency = 0;

    oneapi::tbb::flow::graph g;
    concurrency_functor<int> counter;
    oneapi::tbb::flow::function_node <int, int> fnode(g, oneapi::tbb::flow::serial, counter);

    test_push_receiver<int> sink(g);

    make_edge(fnode, sink);

    for(int i = 0; i < 10; ++i){
        fnode.try_put(i);
    }

    g.wait_for_all();

    CHECK_MESSAGE( ( my_max_concurrency.load() == 1), "Measured parallelism is not expected");
}

template<typename I, typename O>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, function_node<I, O>>::value), "function_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<receiver<I>, function_node<I, O>>::value), "function_node should be derived from receiver<Input>");
    CHECK_MESSAGE( (std::is_base_of<sender<O>, function_node<I, O>>::value), "function_node should be derived from sender<Output>");
}

void test_policy_ctors(){
    using namespace oneapi::tbb::flow;
    graph g;

    function_node<int, int, lightweight> lw_node(g, oneapi::tbb::flow::serial,
                                                          [](int v) { return v;});
    function_node<int, int, queueing_lightweight> qlw_node(g, oneapi::tbb::flow::serial,
                                                          [](int v) { return v;});
    function_node<int, int, rejecting_lightweight> rlw_node(g, oneapi::tbb::flow::serial,
                                                          [](int v) { return v;});

}

class stateful_functor{
public:
    int stored;
    stateful_functor(): stored(-1){}
    int operator()(int value){ stored = 1; return value;}
};
    
void test_ctors(){
    using namespace oneapi::tbb::flow;
    graph g;

    function_node<int, int> fn(g, unlimited, stateful_functor());
    fn.try_put(0);
    g.wait_for_all();

    stateful_functor b1 = copy_body<stateful_functor, function_node<int, int>>(fn);
    CHECK_MESSAGE( (b1.stored == 1), "First node should update");
    
    function_node<int, int> fn2(fn);
    stateful_functor b2 = copy_body<stateful_functor, function_node<int, int>>(fn2);
    CHECK_MESSAGE( (b2.stored == -1), "Copied node should not update");
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

    O operator()(I in){
        return in;
    }
};

void test_copies(){
    using namespace oneapi::tbb::flow;

    CopyCounterBody<int, int> b;

    graph g;
    function_node<int, int> fn(g, unlimited, b);

    CopyCounterBody<int, int> b2 = copy_body<CopyCounterBody<int, int>, function_node<int, int>>(fn);

    CHECK_MESSAGE( (b.copy_count + 2 <= b2.copy_count), "copy_body and constructor should copy bodies");
}

void test_rejecting(){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::function_node <int, int, oneapi::tbb::flow::rejecting> fnode(g, oneapi::tbb::flow::serial,
                                                                    [&](int v){
                                                                        size_t ms = 50;
                                                                        std::chrono::milliseconds sleep_time( ms );
                                                                        std::this_thread::sleep_for( sleep_time );
                                                                        return v;
                                                                    });

    test_push_receiver<int> sink(g);

    make_edge(fnode, sink);

    for(int i = 0; i < 10; ++i){
        fnode.try_put(i);
    }

    g.wait_for_all();
    CHECK_MESSAGE( (get_count(sink) == 1), "Messages should be rejected while the first is being processed");
}

//! Test function_node with rejecting policy
//! \brief \ref interface
TEST_CASE("function_node with rejecting policy"){
    test_rejecting();
}

//! Test body copying and copy_body logic
//! \brief \ref interface
TEST_CASE("function_node and body copying"){
    test_copies();
}

//! Test constructors
//! \brief \ref interface
TEST_CASE("function_node constructors"){
    test_policy_ctors();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("function_node superclasses"){
    test_inheritance<int, int>();
    test_inheritance<void*, float>();
}

//! Test function_node buffering
//! \brief \ref requirement
TEST_CASE("function_node buffering"){
    test_buffering<oneapi::tbb::flow::rejecting>();
    test_buffering<oneapi::tbb::flow::queueing>();
}

//! Test function_node broadcasting
//! \brief \ref requirement
TEST_CASE("function_node broadcast"){
    test_broadcast();
}

//! Test deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE("Deduction guides"){
#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
    test_deduction_guides();
#endif
}

//! Test priorities work in single-threaded configuration
//! \brief \ref requirement
TEST_CASE("function_node priority support"){
    test_priority();
}

//! Test that measured concurrency respects set limits
//! \brief \ref requirement
TEST_CASE("concurrency follows set limits"){
    test_node_concurrency();
}

//! Test calling function body
//! \brief \ref interface \ref requirement
TEST_CASE("Test function_node body") {
    test_func_body();
}
