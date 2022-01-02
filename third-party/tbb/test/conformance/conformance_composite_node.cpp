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

#include "conformance_flowgraph.h"

//! \file conformance_composite_node.cpp
//! \brief Test for [flow_graph.composite_node] specification

class adder : public oneapi::tbb::flow::composite_node<std::tuple<int, int>, std::tuple<int>> {
    oneapi::tbb::flow::join_node<std::tuple<int,int>, oneapi::tbb::flow::queueing> j;
    oneapi::tbb::flow::function_node<std::tuple<int,int>, int> f;
    oneapi::tbb::flow::queue_node<int> qn;
    using base_type = oneapi::tbb::flow::composite_node<std::tuple<int,int>, std::tuple<int>>;

    struct f_body {
        int operator()(const std::tuple<int,int> &t) {
            int sum = std::get<0>(t) + std::get<1>(t);
            return  sum;
        }
    };

public:
    adder(oneapi::tbb::flow::graph &g) : base_type(g), j(g), f(g, oneapi::tbb::flow::unlimited, f_body()), qn(g) {
        make_edge(j, f);
        make_edge(f, qn);

        base_type::input_ports_type input_tuple(oneapi::tbb::flow::input_port<0>(j), oneapi::tbb::flow::input_port<1>(j));
        base_type::output_ports_type output_tuple(qn);
        base_type::set_external_ports(input_tuple, output_tuple);
    }
};

template<int N, typename T1, typename T2>
struct compare {
    static void compare_refs(T1 tuple1, T2 tuple2) {
    CHECK_MESSAGE(( &std::get<N>(tuple1) == &std::get<N>(tuple2)), "ports not set correctly");
    compare<N-1, T1, T2>::compare_refs(tuple1, tuple2);
    }
};

template<typename T1, typename T2>
struct compare<1, T1, T2> {
    static void compare_refs(T1 tuple1, T2 tuple2) {
    CHECK_MESSAGE((&std::get<0>(tuple1) == &std::get<0>(tuple2)), "port 0 not correctly set");
    }
};

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("composite_node superclasses"){
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::graph_node, adder>::value), "composite_node should be derived from graph_node");
}

//! Test composite_node ports
//! \brief \ref interface \ref requirement
TEST_CASE("composite_node ports"){
    oneapi::tbb::flow::graph g;

    using InputTupleType = std::tuple<tbb::flow::continue_msg, std::tuple<int, int>, int, int, int, int,
                             int, int, int, int, int, int, int, int>;

    using OutputTupleType = std::tuple<tbb::flow::continue_msg, std::tuple<int, int>, oneapi::tbb::flow::tagged_msg<size_t, int, float>,
                             int, int, int, int, int, int, int, int, int, int, int, int>;

    using EmptyTupleType= std::tuple<>;

    using input_output_type = oneapi::tbb::flow::composite_node<InputTupleType, OutputTupleType>;
    using input_only_type = oneapi::tbb::flow::composite_node<InputTupleType, EmptyTupleType>;
    using output_only_type = oneapi::tbb::flow::composite_node<EmptyTupleType, OutputTupleType>;

    const size_t NUM_INPUTS = std::tuple_size<InputTupleType>::value;
    const size_t NUM_OUTPUTS = std::tuple_size<OutputTupleType>::value;

    using body = conformance::dummy_functor<int>;

    //node types
    oneapi::tbb::flow::continue_node<tbb::flow::continue_msg> ct(g, body());
    oneapi::tbb::flow::split_node< std::tuple<int, int> > s(g);
    oneapi::tbb::flow::input_node<int> src(g, body());
    oneapi::tbb::flow::function_node<int, int> fxn(g, oneapi::tbb::flow::unlimited, body());
    oneapi::tbb::flow::multifunction_node<int, std::tuple<int, int> > m_fxn(g, oneapi::tbb::flow::unlimited, body());
    oneapi::tbb::flow::broadcast_node<int> bc(g);
    oneapi::tbb::flow::limiter_node<int> lim(g, 2);
    oneapi::tbb::flow::indexer_node<int, float> ind(g);
    oneapi::tbb::flow::join_node< std::tuple< int, int >, oneapi::tbb::flow::queueing > j(g);
    oneapi::tbb::flow::queue_node<int> q(g);
    oneapi::tbb::flow::buffer_node<int> bf(g);
    oneapi::tbb::flow::priority_queue_node<int> pq(g);
    oneapi::tbb::flow::write_once_node<int> wo(g);
    oneapi::tbb::flow::overwrite_node<int> ovw(g);
    oneapi::tbb::flow::sequencer_node<int> seq(g, conformance::sequencer_functor<int>());

    auto input_tuple = std::tie(ct, s, m_fxn, fxn, bc, oneapi::tbb::flow::input_port<0>(j), lim, q, oneapi::tbb::flow::input_port<0>(ind),
                                pq, ovw, wo, bf, seq);
    auto output_tuple = std::tie(ct,j, ind, fxn, src, bc, oneapi::tbb::flow::output_port<0>(s), lim, oneapi::tbb::flow::output_port<0>(m_fxn),
                                 q, pq, ovw, wo, bf, seq);

    //composite_node with both input_ports and output_ports
    input_output_type a_node(g);
    a_node.set_external_ports(input_tuple, output_tuple);

    a_node.add_visible_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);
    a_node.add_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);

    auto a_node_input_ports_ptr = a_node.input_ports();
    compare<NUM_INPUTS-1, decltype(a_node_input_ports_ptr), decltype(input_tuple)>::compare_refs(a_node_input_ports_ptr, input_tuple);
    CHECK_MESSAGE(NUM_INPUTS == std::tuple_size<decltype(a_node_input_ports_ptr)>::value, "not all declared input ports were bound to nodes");

    auto a_node_output_ports_ptr = a_node.output_ports();
    compare<NUM_OUTPUTS-1, decltype(a_node_output_ports_ptr), decltype(output_tuple)>::compare_refs(a_node_output_ports_ptr, output_tuple);
    CHECK_MESSAGE((NUM_OUTPUTS == std::tuple_size<decltype(a_node_output_ports_ptr)>::value), "not all declared output ports were bound to nodes");

    //composite_node with only input_ports
    input_only_type b_node(g);
    b_node.set_external_ports(input_tuple);

    b_node.add_visible_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);
    b_node.add_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);

    auto b_node_input_ports_ptr = b_node.input_ports();
    compare<NUM_INPUTS-1, decltype(b_node_input_ports_ptr), decltype(input_tuple)>::compare_refs(b_node_input_ports_ptr, input_tuple);
    CHECK_MESSAGE(NUM_INPUTS == std::tuple_size<decltype(b_node_input_ports_ptr)>::value, "not all declared input ports were bound to nodes");

    //composite_node with only output_ports
    output_only_type c_node(g);
    c_node.set_external_ports(output_tuple);

    // Reset is not suppose to do anything. Check that it can be called.
    g.reset();

    c_node.add_visible_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);

    c_node.add_nodes(src, fxn, m_fxn, bc, lim, ind, s, ct, j, q, bf, pq, wo, ovw, seq);

    auto c_node_output_ports_ptr = c_node.output_ports();
    compare<NUM_OUTPUTS-1, decltype(c_node_output_ports_ptr), decltype(output_tuple)>::compare_refs(c_node_output_ports_ptr, output_tuple);
    CHECK_MESSAGE(NUM_OUTPUTS == std::tuple_size<decltype(c_node_output_ports_ptr)>::value, "not all declared input ports were bound to nodes");
}

//! Test composite_node construction and message passing
//! \brief \ref interface \ref requirement
TEST_CASE("composite_node construction and message test"){
    using namespace oneapi::tbb::flow;
    graph g;
    split_node<std::tuple<int, int, int, int>> s(g);
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
