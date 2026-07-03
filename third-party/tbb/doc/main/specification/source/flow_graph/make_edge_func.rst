.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
make_edge
=========
**[flow_graph.make_edge]**

A function template for building edges between nodes.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<typename Message>
        inline void make_edge( sender<Message> &p, receiver<Message> &s );

        template< typename MultiOutputNode, typename MultiInputNode >
        inline void make_edge( MultiOutputNode& output, MultiInputNode& input );

        template<typename MultiOutputNode, typename Message>
        inline void make_edge( MultiOutputNode& output, receiver<Message> input );

        template<typename Message, typename MultiInputNode>
        inline void make_edge( sender<Message> output, MultiInputNode& input );

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The `MultiOutputNode` type must have a valid ``MultiOutputNode::output_ports_type`` qualified-id
  that denotes a type.
* The `MultiInputNode` type must have a valid ``MultiInputNode::input_ports_type`` qualified-id
  that denotes a type.

The common form of ``make_edge(sender, receiver)`` creates an edge between provided ``sender``
and ``receiver`` instances.

Overloads that accept a `MultiOutputNode` type instance make an edge from port ``0`` of a
multi-output predecessor.

Overloads that accept a `MultiInputNode` type instance make an edge to port ``0`` of a multi-input
successor.
