.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
graph_node
==========
**[flow_graph.graph_node]**

A base class for all graph nodes.

.. code:: cpp

    namespace oneapi {
    namespace tbb {
    namespace flow {
    
        class graph_node {
        public:
            explicit graph_node( graph &g );
            virtual ~graph_node();
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

The ``graph_node`` class is a base class for all flow graph nodes.
The virtual destructor allows flow graph nodes to be destroyed through pointers to ``graph_node``.
For example, a ``vector< graph_node * >`` can be used to hold the addresses of flow graph nodes
that will need to be destroyed later.
