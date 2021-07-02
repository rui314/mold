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

//! \file conformance_join_node.cpp
//! \brief Test for [flow_graph.join_node] specification

/*
TODO: implement missing conformance tests for join_node:
  - [ ] Check that `OutputTuple' is an instantiation of a tuple.
  - [ ] The copy constructor and copy assignment are called for each type within the `OutputTuple'.
  - [ ] Check all possible policies of the node: `reserving', `key_matching', `queueing',
    `tag_matching'. Check the semantics the node has with each policy separately.
  - [ ] Check that corresponding methods are invoked in specified `KHash' type.
  - [ ] Improve test for constructors, including their availability based on used Policy for the
    node.
  - [ ] Unify code style in the test by extracting the implementation from the `TEST_CASE' scope
    into separate functions.
  - [ ] Check that corresponding methods mentioned in the requirements are called for `Bi' types.
  - [ ] Explicitly check that `input_ports_type' is defined, accessible and is a tuple of
    corresponding to `OutputTuple' receivers.
  - [ ] Explicitly check the method `join_node::input_ports()' exists, is accessible and it returns
    a reference to the `input_ports_type' type.
  - [ ] Implement `test_buffering' (for node policy).
  - [ ] Check `try_get()' copies the generated tuple into passed argument and returns `true'. If
    node is empty returns `false'.
  - [ ] Check `tag_value' is defined and has properties specified.
  - [ ] Add test for CTAD.
*/

using namespace oneapi::tbb::flow;
using namespace std;

template<typename T>
void test_inheritance(){
    CHECK_MESSAGE( (std::is_base_of<graph_node, join_node<std::tuple<T, T>>>::value), "join_node should be derived from graph_node");
    CHECK_MESSAGE( (std::is_base_of<sender<std::tuple<T, T>>, join_node<std::tuple<T, T>>>::value), "join_node should be derived from graph_node");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    join_node<std::tuple<int, int>> n(g);
    join_node<std::tuple<int, int>> n2(n);

    join_node <std::tuple<int, int, oneapi::tbb::flow::reserving>> nr(g);
    join_node <std::tuple<int, int, oneapi::tbb::flow::reserving>> nr2(nr);
}

void test_forwarding(){
    oneapi::tbb::flow::graph g;

    join_node<std::tuple<int, int>> node1(g);

    using output_t = join_node<std::tuple<int, int>>::output_type;

    test_push_receiver<output_t> node2(g);
    test_push_receiver<output_t> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    input_port<0>(node1).try_put(1);
    input_port<1>(node1).try_put(1);

    g.wait_for_all();

    CHECK_MESSAGE( (get_count(node2) == 1), "Descendant of the node needs to be receive N messages");
    CHECK_MESSAGE( (get_count(node3) == 1), "Descendant of the node must receive one message.");
}

//! Test broadcast
//! \brief \ref interface
TEST_CASE("join_node broadcast") {
    test_forwarding();
}


//! Test copy constructor
//! \brief \ref interface
TEST_CASE("join_node copy constructor") {
    test_copies();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("join_node inheritance"){
    test_inheritance<int>();
}

//! Test join_node behavior
//! \brief \ref requirement
TEST_CASE("join_node") {
    graph g;
    function_node<int,int>
        f1( g, unlimited, [](const int &i) { return 2*i; } );
    function_node<float,float>
        f2( g, unlimited, [](const float &f) { return f/2; } );

    join_node< std::tuple<int,float> > j(g);

    function_node< std::tuple<int,float> >
        f3( g, unlimited,
            []( const std::tuple<int,float> &t ) {
                CHECK_MESSAGE( (std::get<0>(t) == 6), "Expected to receive 6" );
                CHECK_MESSAGE( (std::get<1>(t) == 1.5), "Expected to receive 1.5" );
            } );

    make_edge( f1, input_port<0>( j ) );
    make_edge( f2, input_port<1>( j ) );
    make_edge( j, f3 );

    f1.try_put( 3 );
    f2.try_put( 3 );
    g.wait_for_all( );
}

//! Test join_node key matching behavior
//! \brief \ref requirement
TEST_CASE("remove edge to join_node"){
    graph g;
    continue_node<int> c(g, [](const continue_msg&){ return 1; });
    join_node<tuple<int> > jn(g);
    queue_node<tuple<int> > q(g);

    make_edge(jn, q);

    make_edge(c, jn);

    c.try_put(continue_msg());
    g.wait_for_all();

    tuple<int> tmp = tuple<int>(0);
    CHECK_MESSAGE( (q.try_get(tmp)== true), "Message should pass when edge exists");
    CHECK_MESSAGE( (tmp == tuple<int>(1) ), "Message should pass when edge exists");
    CHECK_MESSAGE( (q.try_get(tmp)== false), "Message should not pass after item is consumed");

    remove_edge(c, jn);

    c.try_put(continue_msg());
    g.wait_for_all();

    tmp = tuple<int>(0);
    CHECK_MESSAGE( (q.try_get(tmp)== false), "Message should not pass when edge doesn't exist");
    CHECK_MESSAGE( (tmp == tuple<int>(0)), "Value should not be altered");
}

//! Test join_node key matching behavior
//! \brief \ref requirement
TEST_CASE("join_node key_matching"){
    graph g;
    auto body1 = [](const continue_msg &) -> int { return 1; };
    auto body2 = [](const double &val) -> int { return int(val); };

    join_node<std::tuple<continue_msg, double>, key_matching<int>> jn(g, body1, body2);

    input_port<0>(jn).try_put(continue_msg());
    input_port<1>(jn).try_put(1.3);

    g.wait_for_all( );

    tuple<continue_msg, double> tmp;
    CHECK_MESSAGE( (jn.try_get(tmp) == true), "Mapped keys should match");
}
