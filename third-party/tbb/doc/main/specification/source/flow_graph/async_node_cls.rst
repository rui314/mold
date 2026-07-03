.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
async_node
==========
**[flow_graph.async_node]**

A node that enables communication between a flow graph and an external activity managed by
the user or another runtime.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template < typename Input, typename Output, typename Policy = /*implemetation-defined*/ >
        class async_node : public graph_node, public receiver<Input>, public sender<Output> {
        public:
            template<typename Body>
            async_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                        node_priority_t priority = no_priority );
            template<typename Body>
            async_node( graph &g, size_t concurrency, Body body, node_priority_t priority = no_priority );

            async_node( const async_node& src );
            ~async_node();

            using gateway_type = /*implementation-defined*/;
            gateway_type& gateway();

            bool try_put( const input_type& v );
            bool try_get( output_type& v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Input`` type must meet the `DefaultConstructible` requirements from [defaultconstructible]
  and the `CopyConstructible` requirements from [copyconstructible] ISO C++ Standard sections.
* The type ``Policy`` can be specified as :doc:`lightweight, queueing and rejecting policies<functional_node_policies>` or defaulted.
* The type ``Body`` must meet the :doc:`AsyncNodeBody requirements <../named_requirements/flow_graph/async_node_body>`.
  Since C++17, ``Body`` may also be a pointer to a const member function in ``Input`` that takes ``gateway_type&`` argument.

``async_node`` executes a user-provided body on incoming messages. The body typically submits the
messages to an external activity for processing outside of the graph. It is responsibility of
``body`` to be able to pass the message to an external activity. This node also provides the
``gateway_type`` interface that allows an external activity to communicate with the flow graph.

``async_node`` is a ``graph_node``, ``receiver<Input>``, and a ``sender<Output>``.

``async_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

``async_node`` has a user-settable concurrency limit, which can be set to one of :doc:`predefined values <predefined_concurrency_limits>`.
The user can also provide a value of type ``std::size_t`` to limit concurrency to a value between 1 and
:doc:`tbb::flow::unlimited <predefined_concurrency_limits>`.

The body object passed to a ``async_node`` is copied. Updates to member variables do not affect the original object used to construct the node.
If the state held within a body object must be inspected from outside of the node,
the :doc:`copy_body <copy_body_func>` function can be used to obtain an updated copy.

Member types
----------------

``gateway_type`` meets the :doc:`GatewayType requirements <../named_requirements/flow_graph/gateway_type>`.

Member functions
----------------

.. code:: cpp

    template<typename Body>
    async_node( graph &g, size_t concurrency, Body body,
                   node_priority_t priority = no_priority );

Constructs an ``async_node`` that invokes a copy of ``body``. The ``concurrency`` value limits the number of simultaneous
``body`` invocations for the node.

This function specifies :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    async_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                   node_priority_t priority = no_priority );

Constructs a ``async_node`` that invokes a copy of ``body``. Most ``concurrency`` calls
to ``body`` can be made concurrently.

This function specifies a :doc:`policy<functional_node_policies>` and :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    async_node( const async_node &src )

Constructs an ``async_node`` that has the same initial state that ``src`` had when it was
constructed. The ``async_node`` that is constructed has a reference to the same ``graph``
object as ``src``, has a copy of the initial body used by ``src``, and has the same
concurrency threshold as ``src``. The predecessors and successors of ``src`` are not copied.

The new body object is copy-constructed from a copy of the original body provided to ``src`` at
its construction. Changes made to member variables in ``src``'s body after the
construction of ``src`` do not affect the body of the new ``async_node.``

----------------------------------------------------------------

.. code:: cpp

    gateway_type& gateway()

Returns reference to the ``gateway_type`` interface.

----------------------------------------------------------------

.. code:: cpp

    bool try_put( const input_type& v )

If the concurrency limit allows, executes the user-provided body on the incoming message ``v``.
Otherwise, depending on the policy of the node, either queues the incoming message ``v`` or rejects
it.

**Returns:** ``true`` if the input was accepted; and ``false``, otherwise.

----------------------------------------------------------------

.. code:: cpp

    bool try_get( output_type& v )

**Returns**: ``false``
