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

//! \file conformance_indexer_node.cpp
//! \brief Test for [flow_graph.indexer_node] specification

/*
TODO: implement missing conformance tests for buffer_node:
  - [ ] The copy constructor is called for the node's type template parameter.
  - [ ] Improve `test_forwarding' by checking that the value passed is the actual one received.
  - [ ] Improve `test_buffering' by checking that additional `try_get()' does not receive the same value.
  - [ ] Improve tests of the constructors.
  - [ ] Based on the decision about the details for `try_put()' and `try_get()' write corresponding tests.
  - [ ] Fix description in `TEST_CASEs'.
*/
using namespace oneapi::tbb::flow;
using namespace std;

template<typename I1, typename I2>
void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, indexer_node<I1, I2>>::value), "indexer_node should be derived from graph_node");
}

void test_copies(){
    using namespace oneapi::tbb::flow;

    graph g;
    indexer_node<int, int> fn(g);

    indexer_node<int, int> f2(fn);
}

//! Test body copying and copy_body logic
//! \brief \ref interface
TEST_CASE("indexer_node and body copying"){
    test_copies();
}

void test_broadcasting(){
    oneapi::tbb::flow::graph g;

    typedef indexer_node<int,float> my_indexer_type;
    typedef my_indexer_type::output_type my_output_type;

    my_indexer_type o(g);

    my_indexer_type node1(g);
    queue_node<my_output_type> node2(g);
    queue_node<my_output_type> node3(g);

    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::make_edge(node1, node3);

    input_port<0>(node1).try_put(6);
    input_port<1>(node1).try_put(1.5);
    g.wait_for_all();

    my_output_type tmp;
    CHECK_MESSAGE( (node2.try_get(tmp)), "Descendant of the node needs to receive message once");
    CHECK_MESSAGE( (node3.try_get(tmp)), "Descendant of the node needs to receive message once");
}

//! Test broadcasting property
//! \brief \ref requirement
TEST_CASE("indexer_node broadcasts"){
    test_broadcasting();
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("indexer_node superclasses"){
    test_inheritance<int, int>();
}

//! Test discarding property
//! \brief \ref requirement
TEST_CASE("indexer_node discarding") {
  graph g;

  typedef indexer_node<int,float> my_indexer_type;
  my_indexer_type o(g);

  limiter_node< my_indexer_type::output_type > rejecter( g,0);
  make_edge( o, rejecter );

  input_port<0>(o).try_put(6);
  input_port<1>(o).try_put(1.5);

  my_indexer_type::output_type tmp;
  CHECK_MESSAGE((o.try_get(tmp) == false), "Value should be discarded after rejection");
  g.wait_for_all();
}

//! Test indexer body
//! \brief \ref requirement
TEST_CASE("indexer_node body") {
  graph g;
  function_node<int,int> f1( g, unlimited,
                               [](const int &i) { return 2*i; } );
  function_node<float,float> f2( g, unlimited,
                               [](const float &f) { return f/2; } );

  typedef indexer_node<int,float> my_indexer_type;
  my_indexer_type o(g);

  function_node< my_indexer_type::output_type >
    f3( g, unlimited,
        []( const my_indexer_type::output_type &v ) {
            if (v.tag() == 0) {
                CHECK_MESSAGE( (cast_to<int>(v) == 6), "Expected to receive 6" );
            } else {
                CHECK_MESSAGE( (cast_to<float>(v) == 1.5), "Expected to receive 1.5" );
           }
        }
    );
  make_edge( f1, input_port<0>(o) );
  make_edge( f2, input_port<1>(o) );
  make_edge( o, f3 );

  f1.try_put( 3 );
  f2.try_put( 3 );
  g.wait_for_all();
}
