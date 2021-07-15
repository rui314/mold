/*
    Copyright (c) 2005-2021 Intel Corporation

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

#ifndef TBB_examples_logic_sim_oba_H
#define TBB_examples_logic_sim_oba_H

namespace P {
//input ports
const int CI = 0;
const int A0 = 1;
const int B0 = 2;
const int A1 = 3;
const int B1 = 4;
const int A2 = 5;
const int B2 = 6;
const int A3 = 7;
const int B3 = 8;

//output_ports
const int S0 = 0;
const int S1 = 1;
const int S2 = 2;
const int S3 = 3;

#if USE_TWO_BIT_FULL_ADDER
const int CO = 2;
#else
const int CO = 4;
#endif
} // namespace P

#include "basics.hpp"

class one_bit_adder
        : public oneapi::tbb::flow::composite_node<std::tuple<signal_t, signal_t, signal_t>,
                                                   std::tuple<signal_t, signal_t>> {
    oneapi::tbb::flow::broadcast_node<signal_t> A_port;
    oneapi::tbb::flow::broadcast_node<signal_t> B_port;
    oneapi::tbb::flow::broadcast_node<signal_t> CI_port;
    xor_gate<2> FirstXOR;
    xor_gate<2> SecondXOR;
    and_gate<2> FirstAND;
    and_gate<2> SecondAND;
    or_gate<2> FirstOR;
    oneapi::tbb::flow::graph& my_graph;
    typedef oneapi::tbb::flow::composite_node<std::tuple<signal_t, signal_t, signal_t>,
                                              std::tuple<signal_t, signal_t>>
        base_type;

public:
    one_bit_adder(oneapi::tbb::flow::graph& g)
            : base_type(g),
              my_graph(g),
              A_port(g),
              B_port(g),
              CI_port(g),
              FirstXOR(g),
              SecondXOR(g),
              FirstAND(g),
              SecondAND(g),
              FirstOR(g) {
        make_connections();
        set_up_composite();
    }
    one_bit_adder(const one_bit_adder& src)
            : base_type(src.my_graph),
              my_graph(src.my_graph),
              A_port(src.my_graph),
              B_port(src.my_graph),
              CI_port(src.my_graph),
              FirstXOR(src.my_graph),
              SecondXOR(src.my_graph),
              FirstAND(src.my_graph),
              SecondAND(src.my_graph),
              FirstOR(src.my_graph) {
        make_connections();
        set_up_composite();
    }

    ~one_bit_adder() {}

private:
    void make_connections() {
        make_edge(A_port, input_port<0>(FirstXOR));
        make_edge(A_port, input_port<0>(FirstAND));
        make_edge(B_port, input_port<1>(FirstXOR));
        make_edge(B_port, input_port<1>(FirstAND));
        make_edge(CI_port, input_port<1>(SecondXOR));
        make_edge(CI_port, input_port<1>(SecondAND));
        make_edge(FirstXOR, input_port<0>(SecondXOR));
        make_edge(FirstXOR, input_port<0>(SecondAND));
        make_edge(SecondAND, input_port<0>(FirstOR));
        make_edge(FirstAND, input_port<1>(FirstOR));
    }

    void set_up_composite() {
        base_type::input_ports_type input_tuple(CI_port, A_port, B_port);
        base_type::output_ports_type output_tuple(output_port<0>(SecondXOR),
                                                  output_port<0>(FirstOR));
        base_type::set_external_ports(input_tuple, output_tuple);
        base_type::add_visible_nodes(
            A_port, B_port, CI_port, FirstXOR, SecondXOR, FirstAND, SecondAND, FirstOR);
    }
};

#endif /* TBB_examples_logic_sim_oba_H */
