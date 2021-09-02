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

#ifndef TBB_examples_logic_sim_tba_H
#define TBB_examples_logic_sim_tba_H

#include "one_bit_adder.hpp"

class two_bit_adder : public oneapi::tbb::flow::composite_node<
                          std::tuple<signal_t, signal_t, signal_t, signal_t, signal_t>,
                          std::tuple<signal_t, signal_t, signal_t>> {
    oneapi::tbb::flow::graph& my_graph;
    std::vector<one_bit_adder> two_adders;
    typedef oneapi::tbb::flow::composite_node<
        std::tuple<signal_t, signal_t, signal_t, signal_t, signal_t>,
        std::tuple<signal_t, signal_t, signal_t>>
        base_type;

public:
    two_bit_adder(oneapi::tbb::flow::graph& g)
            : base_type(g),
              my_graph(g),
              two_adders(2, one_bit_adder(g)) {
        make_connections();
        set_up_composite();
    }
    two_bit_adder(const two_bit_adder& src)
            : base_type(src.my_graph),
              my_graph(src.my_graph),
              two_adders(2, one_bit_adder(src.my_graph)) {
        make_connections();
        set_up_composite();
    }
    ~two_bit_adder() {}

private:
    void make_connections() {
        make_edge(output_port<1>(two_adders[0]), input_port<0>(two_adders[1]));
    }
    void set_up_composite() {
        base_type::input_ports_type input_tuple(input_port<0>(two_adders[0] /*CI*/),
                                                input_port<1>(two_adders[0]),
                                                input_port<2>(two_adders[0]),
                                                input_port<1>(two_adders[1]),
                                                input_port<2>(two_adders[1]));

        base_type::output_ports_type output_tuple(output_port<0>(two_adders[0]),
                                                  output_port<0>(two_adders[1]),
                                                  output_port<1>(two_adders[1] /*CO*/));
        base_type::set_external_ports(input_tuple, output_tuple);
    }
};

#endif /* TBB_examples_logic_sim_tba_H */
