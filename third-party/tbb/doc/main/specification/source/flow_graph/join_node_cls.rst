.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
join_node
=========
**[flow_graph.join_node]**

A node that creates a tuple from a set of messages received at its input ports and broadcasts the
tuple to all of its successors.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {
        using tag_value = /*implementation-specific*/;

        template<typename OutputTuple, class JoinPolicy = /*implementation-defined*/>
        class join_node : public graph_node, public sender< OutputTuple > {
        public:
            using input_ports_type = /*implementation-defined*/;

            explicit join_node( graph &g );
            join_node( const join_node &src );

            input_ports_type &input_ports( );

            bool try_get( OutputTuple &v );
        };

        template<typename OutputTuple, typename K, class KHash=tbb_hash_compare<K> >
        class join_node< OutputTuple, key_matching<K,KHash> > : public graph_node, public sender< OutputTuple > {
        public:
            using input_ports_type = /*implementation-defined*/;

            explicit join_node( graph &g );
            join_node( const join_node &src );

            template <typename B0, typename... BN>
            join_node( graph &g, B0 b0, BN... bn );

            input_ports_type &input_ports( );

            bool try_get( OutputTuple &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``OutputTuple`` must be an instantiation of ``std::tuple``. Each type that the tuple
  stores must meet the `DefaultConstructible` requirements from [defaultconstructible],
  `CopyConstructible` requirements from [copyconstructible] and `CopyAssignable` requirements from
  [copyassignable] ISO C++ Standard sections.
* The ``JoinPolicy`` type must be specified as one of :doc:`buffering policies <join_node_policies>` for ``join_node``.
* The ``KHash`` type must meet the :doc:`HashCompare requirements <../named_requirements/containers/hash_compare>`.
* The ``Bi`` types must meet the :doc:`JoinNodeFunctionObject requirements <../named_requirements/flow_graph/join_node_func_obj>`.
  Since C++17, each of ``Bi`` types may also be a pointer to a const member function in ``Input`` that returns ``Key`` or
  a pointer to a data member of type ``Key`` in ``Input``.

A ``join_node`` is a ``graph_node`` and a ``sender<OutputTuple>``.
It contains a tuple of input ports, each of which is a ``receiver<Type>`` for each `Type` in
``OutputTuple``. It supports multiple input receivers with distinct types and broadcasts a tuple
of received messages to all of its successors. All input ports of a ``join_node`` must use the same
buffering policy.

The behavior of a ``join_node`` is based on its buffering policy.

.. toctree::
    :titlesonly:

    join_node_policies.rst


The function template :doc:`input_port <input_port_func>` simplifies the syntax for getting a
reference to a specific input port.

``join_node`` has a `buffering` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

Member types
------------

``input_ports_type`` is an alias to a tuple of input ports.

Member functions
----------------

.. code:: cpp

    explicit join_node( graph &g );

Constructs an empty ``join_node`` that belongs to the graph ``g``.

----------------------------------------------------------------

.. code:: cpp

    template <typename B0, typename... BN>
    join_node( graph &g, B0 b0, BN... bn );

A constructor only available in the ``key_matching`` specialization of ``join_node``.

Creates a ``join_node`` that uses the function objects ``b0``, ... , ``bN`` to determine
the tags for the input ports ``0`` through ``N``.

**Constraints:**: only participates in overload resolution if ``std::tuple_size<OutputTuple>::value`` is ``1 + sizeof...(BN)``.

.. caution::

    Function objects passed to the join_node constructor must not
    throw. They are called in parallel, and should be pure, take minimal
    time, and be non-blocking.

----------------------------------------------------------------

.. code:: cpp

    join_node( const join_node &src )

Creates a ``join_node`` that has the same initial state that
``src`` had at its construction. The list of predecessors,
messages in the input ports, and successors are not copied.

----------------------------------------------------------------

.. code:: cpp

    input_ports_type &input_ports( )

**Returns**: a ``std::tuple`` of receivers. Each element inherits values from ``receiver<T>``, where
``T`` is the type of message expected at that input. Each tuple element can be used like any
other ``receiver<T>``. The behavior of the ports is based on the selected ``join_node`` policy.

----------------------------------------------------------------

.. code:: cpp

    bool try_get( output_type &v )

Attempts to generate a tuple based on the buffering policy of the ``join_node``.

If it can successfully generate a tuple, it copies it to ``v`` and returns ``true``.
Otherwise, it returns ``false``.

Non-Member Types
----------------

.. code:: cpp

    using tag_value = /*implementation-specific*/;

``tag_value`` is an unsigned integral type for defining the ``tag_matching`` policy.

Deduction Guides
----------------

.. code:: cpp

    template <typename Body, typename... Bodies>
    join_node(graph&, Body, Bodies...)
        ->join_node<std::tuple<std::decay_t<input_t<Body>>, std::decay_t<input_t<Bodies>>...>, key_matching<output_t<Body>>>;

Where:

* ``input_t`` is an alias to the input argument type of the passed function object.
* ``output_t`` is an alias to the return type of the passed function object.
