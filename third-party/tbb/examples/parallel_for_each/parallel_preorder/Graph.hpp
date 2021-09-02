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

#ifndef TBB_examples_parallel_preorder_graph_H
#define TBB_examples_parallel_preorder_graph_H

#include <vector>
#include <atomic>

#include "Matrix.hpp"

enum OpKind {
    // Use Cell's value
    OP_VALUE,
    // Unary negation
    OP_NEGATE,
    // Addition
    OP_ADD,
    // Subtraction
    OP_SUB,
    // Multiplication
    OP_MUL
};

static const int ArityOfOp[] = { 0, 1, 2, 2, 2 };

class Cell {
public:
    //! Operation for this cell
    OpKind op;

    //! Inputs to this cell
    Cell* input[2];

    //! Type of value stored in a Cell
    typedef Matrix value_type;

    //! Value associated with this Cell
    value_type value;

    //! Set of cells that use this Cell as an input
    std::vector<Cell*> successor;

    //! Reference count of number of inputs that are not yet updated.
    std::atomic<int> ref_count;

    //! Update the Cell's value.
    void update();

    //! Default constructor
    Cell() {}

    //! Copy constructor
    Cell(const Cell& other);
};

//! A directed graph where the vertices are Cells.
class Graph {
    std::vector<Cell> my_vertex_set;

public:
    //! Create a random acyclic directed graph
    void create_random_dag(std::size_t number_of_nodes);

    //! Print the graph
    void print();

    //! Get set of cells that have no inputs.
    void get_root_set(std::vector<Cell*>& root_set);
};

#endif /* TBB_examples_parallel_preorder_graph_H */
