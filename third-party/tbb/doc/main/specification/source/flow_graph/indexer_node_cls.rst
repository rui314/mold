.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
indexer_node
============
**[flow_graph.indexer_node]**

``indexer_node`` broadcasts messages received at input ports to
all of its successors. The messages are broadcast individually as they
are received at each port. The output is a :doc:`tagged message <tagged_msg_cls>`
that contains a tag and a value; the tag identifies the input port on
which the message was received.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<typename T0, typename... TN>
        class indexer_node : public graph_node, public sender</*implementation_defined*/> {
        public:
            indexer_node(graph &g);
            indexer_node(const indexer_node &src);

            using input_ports_type = /*implementation_defined*/;
            input_ports_type &input_ports();

            using output_type = tagged_msg<size_t, T0, TN...>;
            bool try_get( output_type &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``T0`` type and all types in ``TN`` template parameter pack must meet the
  `CopyConstructible` requirements from [copyconstructible] ISO C++ Standard
  section.

An ``indexer_node`` is a ``graph_node`` and ``sender<tagged_msg<size_t, T0, TN...>>``.
It contains a tuple of input ports, each of which is a
``receiver`` specified by corresponding input template parameter pack element. It
supports multiple input receivers with distinct types and broadcasts
each received message to all of its successors. Unlike a
``join_node``, each message is broadcast individually to all
successors of the ``indexer_node`` as it arrives at an input
port. Before broadcasting, a message is tagged with the index of the
port on which the message arrived.

``indexer_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

The function template :doc:`input_port <input_port_func>` simplifies the syntax for getting
a reference to a specific input port.

Member types
------------

* ``input_ports_type`` is an alias to a ``std::tuple`` of input ports.
* ``output_type`` is an alias to the message of type ``tagged_msg``, which is sent to successors.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::indexer_node
	       
.. cpp:function:: indexer_node(graph &g)

    Constructs an ``indexer_node`` that belongs to the graph ``g``.

.. cpp:function:: indexer_node( const indexer_node &src )

    Constructs an ``indexer_node``. The list of predecessors, messages
    in the input ports, and successors are not copied.

.. cpp:function:: input_ports_type& input_ports()

    **Returns**: A ``std::tuple`` of receivers. Each element inherits
    from ``receiver<T>`` where ``T`` is the type of message expected at
    that input. Each tuple element can be used like any other
    ``receiver<T>``.

.. cpp:function:: bool try_get( output_type &v )

    An ``indexer_node`` contains no buffering and therefore does not
    support gets.

    **Returns**: ``false``.

See also:

* :doc:`input_port function template <input_port_func>`
* :doc:`tagged_msg template class <tagged_msg_cls>`
