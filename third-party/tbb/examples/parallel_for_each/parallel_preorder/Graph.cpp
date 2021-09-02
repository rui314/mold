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

#include <cstdlib>

#include <iostream>

#include "Graph.hpp"

Cell::Cell(const Cell& other) : op(other.op), value(other.value), successor(other.successor) {
    ref_count = other.ref_count.load();

    input[0] = other.input[0];
    input[1] = other.input[1];
}

void Graph::create_random_dag(std::size_t number_of_nodes) {
    my_vertex_set.resize(number_of_nodes);
    for (std::size_t k = 0; k < number_of_nodes; ++k) {
        Cell& c = my_vertex_set[k];
        int op = int((rand() >> 8) % 5u);
        if (op > int(k))
            op = int(k);
        switch (op) {
            default:
                c.op = OP_VALUE;
                c.value = Cell::value_type((float)k);
                break;
            case 1: c.op = OP_NEGATE; break;
            case 2: c.op = OP_SUB; break;
            case 3: c.op = OP_ADD; break;
            case 4: c.op = OP_MUL; break;
        }
        for (int j = 0; j < ArityOfOp[c.op]; ++j) {
            Cell& input = my_vertex_set[rand() % k];
            c.input[j] = &input;
        }
    }
}

void Graph::print() {
    for (std::size_t k = 0; k < my_vertex_set.size(); ++k) {
        std::cout << "Cell " << k << ":";
        for (std::size_t j = 0; j < my_vertex_set[k].successor.size(); ++j)
            std::cout << " " << int(my_vertex_set[k].successor[j] - &my_vertex_set[0]);
        std::cout << "\n";
    }
}

void Graph::get_root_set(std::vector<Cell*>& root_set) {
    for (std::size_t k = 0; k < my_vertex_set.size(); ++k) {
        my_vertex_set[k].successor.clear();
    }
    root_set.clear();
    for (std::size_t k = 0; k < my_vertex_set.size(); ++k) {
        Cell& c = my_vertex_set[k];
        c.ref_count = ArityOfOp[c.op];
        for (int j = 0; j < ArityOfOp[c.op]; ++j) {
            c.input[j]->successor.push_back(&c);
        }
        if (ArityOfOp[c.op] == 0)
            root_set.push_back(&my_vertex_set[k]);
    }
}

void Cell::update() {
    switch (op) {
        case OP_VALUE: break;
        case OP_NEGATE: value = -(input[0]->value); break;
        case OP_ADD: value = input[0]->value + input[1]->value; break;
        case OP_SUB: value = input[0]->value - input[1]->value; break;
        case OP_MUL: value = input[0]->value * input[1]->value; break;
    }
}
