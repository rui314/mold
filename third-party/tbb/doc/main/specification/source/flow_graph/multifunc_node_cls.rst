.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
multifunction_node
==================
**[flow_graph.multifunction_node]**

A node that used for nodes that receive messages at a single input port and may generate
one or more messages that are broadcast to successors.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template < typename Input, typename Output, typename Policy = /*implementation-defined*/ >
        class multifunction_node : public graph_node, public receiver<Input> {
        public:
            template<typename Body>
            multifunction_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                                node_priority_t priority = no_priority );
            template<typename Body>
            multifunction_node( graph &g, size_t concurrency, Body body,
                                node_priority_t priority = no_priority );

            multifunction_node( const multifunction_node& other );
            ~multifunction_node();

            bool try_put( const Input &v );

            using output_ports_type = /*implementation-defined*/;
            output_ports_type& output_ports();
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Input`` type must meet the `DefaultConstructible` requirements from [defaultconstructible]
  and the `CopyConstructible` requirements from [copyconstructible] ISO C++ Standard sections.
* The type ``Policy`` can be specified as :doc:`lightweight, queueing and rejecting policies<functional_node_policies>` or defaulted.
* The type ``Body`` must meet the :doc:`MultifunctionNodeBody requirements <../named_requirements/flow_graph/multifunction_node_body>`.
  Since C++17, ``Body`` may also be a pointer to a const member function in ``Input`` that takes ``output_ports_type&`` argument.

``multifunction_node`` has a user-settable concurrency limit. It can be set to one of :doc:`predefined values <predefined_concurrency_limits>`.
The user can also provide a value of type ``std::size_t`` to limit concurrency to a value between 1 and :doc:`tbb::flow::unlimited <predefined_concurrency_limits>`.

When the concurrency limit allows, it executes the user-provided body on incoming messages.
The body can create one or more output messages and broadcast them to successors.

``multifunction_node`` is a ``graph_node``, ``receiver<InputType>`` and has a tuple of
``sender<Output>`` outputs.

``multifunction_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

The body object passed to a ``multifunction_node`` is copied. Updates to member variables do
not affect the original object used to construct the node. If the state held within a body object
must be inspected from outside of the node, the :doc:`copy_body function <copy_body_func>` can be
used to obtain an updated copy.

Member types
------------

``output_ports_type`` is an alias to a ``std::tuple`` of output ports.

Member functions
----------------

.. code:: cpp

    template<typename Body>
    multifunction_node( graph &g, size_t concurrency, Body body,
                        node_priority_t priority = no_priority );

Constructs a ``multifunction_node`` that invokes a copy of ``body``. Most ``concurrency`` calls
to ``body`` can be made concurrently.

Use this function to specify :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    multifunction_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                        node_priority_t priority = no_priority );

Constructs a ``multifunction_node`` that invokes a copy of ``body``. Most ``concurrency`` calls
to ``body`` can be made concurrently.

Use this function to specify a :doc:`policy<functional_node_policies>` and :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    multifunction_node( const multifunction_node &src )

Constructs a ``multifunction_node`` that has the same initial
state that ``other`` had when it was constructed. The
``multifunction_node`` that is constructed has a reference
to the same ``graph`` object as ``other``, has a copy of the
initial ``body`` used by ``other``, and has the same concurrency
threshold as ``other``. The predecessors and successors of
``other`` are not copied.

The new body object is copy-constructed from a copy of the
original body provided to ``other`` at its construction. Changes made to member variables in ``other`` body after the
construction of ``other`` do not affect the body of the new
``multifunction_node.``

----------------------------------------------------------------

.. code:: cpp

    bool try_put( const input_type &v )

If the concurrency limit allows, executes the user-provided body on the incoming message ``v``.
Otherwise, depending on the policy of the node, either queues the incoming message ``v`` or rejects
it.

**Returns:** ``true`` if the input was accepted; ``false``, otherwise.

----------------------------------------------------------------

.. code:: cpp

    output_ports_type& output_ports();

**Returns:** a ``std::tuple`` of output ports.
