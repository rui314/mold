.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
split_node
==========
**[flow_graph.split_node]**

A ``split_node`` sends each element of the incoming ``std::tuple`` to the output port that matches the element index
in the incoming tuple.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template < typename TupleType >
        class split_node : public graph_node, public receiver<TupleType> {
        public:
            explicit split_node( graph &g );
            split_node( const split_node &other );
            ~split_node();

            bool try_put( const TupleType &v );

            using output_ports_type = /*implementation-defined*/ ;
            output_ports_type& output_ports();
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``TupleType`` must be an instantiation of ``std::tuple``.

``split_node`` is a ``receiver<TupleType>`` and has a tuple of ``sender`` output ports. Each of output
ports is specified by corresponding tuple element type. This node receives a tuple at its single input
port and generates a message from each element of the tuple, passing each to the corresponding output port.

``split_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

``split_node`` has unlimited concurrency, and behaves as a ``broadcast_node`` with multiple output ports.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::split_node

.. cpp:function:: explicit split_node( graph &g )

  Constructs a ``split_node`` registered with graph ``g``.

.. cpp:function:: split_node( const split_node &other )

  Constructs a ``split_node`` that has the same initial state that
  ``other`` had when it was constructed. The ``split_node`` that is
  constructed has a reference to the same ``graph`` object as
  ``other``. The predecessors and successors of ``other`` are not
  copied.

.. cpp:function:: ~split_node()

  Destructor

.. cpp:function:: bool try_put( const TupleType &v )

  Broadcasts each element of the incoming tuple to the nodes connected
  to the ``split_node`` output ports. The element at index ``i`` of
  ``v`` will be broadcast through the ``i``\ :sup:`th` output port.

  **Returns**: ``true``

.. cpp:function:: output_ports_type& output_ports()

  **Returns**: a ``std::tuple`` of output ports.
