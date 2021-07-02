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

//! \file conformance_composite_node.cpp
//! \brief Test for [flow_graph.composite_node] specification

/*
TODO: implement missing conformance tests for composite_node:
  - [ ] Check that `input_ports_type' and `output_ports_type' are defined, accessible, and meet
    their requirements, that is, each element is a reference to actual node, input or output
    port respectively.
  - [ ] Add tests for `composite_node' with only input and output ports.
  - [ ] Make sure `input_ports()' and `output_ports()' are defined and accessible in respective
    specializations.
  - [ ] Check the size of input and output tuples is equal to the size of `input_ports_type' and
    `output_ports_type'.
*/

using namespace oneapi::tbb::flow;
using namespace std;

class adder : public composite_node<  tuple<int, int>,  tuple<int> > {
    join_node< tuple<int,int>, queueing > j;
    function_node< tuple<int,int>, int > f;
    queue_node <int> qn;
    typedef composite_node< tuple<int,int>, tuple<int> > base_type;

    struct f_body {
        int operator()(const tuple<int,int> &t) {
            int sum = get<0>(t) + get<1>(t);
            return  sum;
        }
    };

public:
    adder(graph &g) : base_type(g), j(g), f(g, unlimited, f_body()), qn(g) {
        make_edge(j, f);
        make_edge(f, qn);

        base_type::input_ports_type input_tuple(input_port<0>(j), input_port<1>(j));
        base_type::output_ports_type output_tuple(qn);
        base_type::set_external_ports(input_tuple, output_tuple);
    }
};

void test_inheritance(){
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE( (std::is_base_of<graph_node, adder>::value), "multifunction_node should be derived from graph_node");

}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("composite_node superclasses"){
    test_inheritance();
}

//! Test inheritance relations
//! \brief \ref interface \ref requirement
TEST_CASE("Construction and message test"){
    graph g;
    split_node< tuple<int, int, int, int> > s(g);
    adder a0(g);
    adder a1(g);
    adder a2(g);

    make_edge(output_port<0>(s), input_port<0>(a0));
    make_edge(output_port<1>(s), input_port<1>(a0));

    make_edge(output_port<0>(a0),input_port<0>(a1));
    make_edge(output_port<2>(s), input_port<1>(a1));

    make_edge(output_port<0>(a1), input_port<0>(a2));
    make_edge(output_port<3>(s), input_port<1>(a2));

    s.try_put(std::make_tuple(1,3,5,7));
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((output_port<0>(a2).try_get(tmp) == true), "Composite node should produce a value");
    CHECK_MESSAGE((tmp == 1+3+5+7), "Composite node should produce correct sum");
}
