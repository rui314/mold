.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
continue_node
=============
**[flow_graph.continue_node]**

A node that executes a specified body object when triggered.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename Output, typename Policy = /*implementation-defined*/ >
        class continue_node : public graph_node, public receiver<continue_msg>, public sender<Output> {
        public:
            template<typename Body>
            continue_node( graph &g, Body body, node_priority_t priority = no_priority );
            template<typename Body>
            continue_node( graph &g, Body body, Policy /*unspecified*/ = Policy(),
                          node_priority_t priority = no_priority );

            template<typename Body>
            continue_node( graph &g, int number_of_predecessors, Body body,
                           node_priority_t priority = no_priority );
            template<typename Body>
            continue_node( graph &g, int number_of_predecessors, Body body,
                           Policy /*unspecified*/ = Policy(), node_priority_t priority = no_priority );

            continue_node( const continue_node &src );
            ~continue_node();

            bool try_put( const input_type &v );
            bool try_get( output_type &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``Output`` must meet the `CopyConstructible` requirements from [copyconstructible] ISO
  C++ Standard section.
* The type ``Policy`` can be specified as :doc:`lightweight policy<functional_node_policies>` or defaulted.
* The type ``Body`` must meet the :doc:`ContinueNodeBody requirements <../named_requirements/flow_graph/continue_node_body>`.

A ``continue_node`` is a ``graph_node``, ``receiver<continue_msg>``, and ``sender<Output>``.

This node is used for nodes that wait for their predecessors to complete before executing, but no
explicit data is passed across the incoming edges.

A ``continue_node`` maintains an internal threshold that defines the number of predecessors.
This value can be provided at construction. Call of the :doc:`make_edge function <make_edge_func>`
with ``continue_node`` as a receiver increases its threshold. Call of the
:doc:`remove_edge function <remove_edge_func>` with ``continue_node`` as a receiver
decreases it.

Each time the number of ``try_put()`` calls reaches the defined threshold, node's ``body`` is called
and the node starts counting the number of ``try_put()`` calls from the beginning.

``continue_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

The body object passed to a ``continue_node`` is copied. Updates to member variables do
not affect the original object used to construct the node. If the state held within a body object
must be inspected from outside of the node, the :doc:`copy_body function <copy_body_func>` can be
used to obtain an updated copy.


Member functions
-----------------

.. code:: cpp

    template<typename Body>
    continue_node( graph &g, Body body, node_priority_t priority = no_priority );


Constructs a ``continue_node`` that invokes ``body``. The internal threshold is set to 0.

This function specifies :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    continue_node( graph &g, Body body, Policy /*unspecified*/ = Policy(),
                   node_priority_t priority = no_priority );

Constructs a ``continue_node`` that invokes ``body``. The internal threshold is set to 0.

This function specifies :doc:`lightweight policy<functional_node_policies>` and :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    continue_node( graph &g, int number_of_predecessors, Body body,
                   node_priority_t priority = no_priority );

Constructs a ``continue_node`` that invokes ``body``. The internal threshold is set to
``number_of_predecessors``.

This function specifies :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    continue_node( graph &g, int number_of_predecessors, Body body,
                   Policy /*unspecified*/ = Policy(), node_priority_t priority = no_priority );

Constructs a ``continue_node`` that invokes ``body``. The internal threshold is set to
``number_of_predecessors``.

This function specifies :doc:`lightweight policy<functional_node_policies>` and :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    continue_node( graph &g, int number_of_predecessors, Body body );

Constructs a ``continue_node`` that invokes ``body``. The internal threshold is set to
``number_of_predecessors``.

----------------------------------------------------------------

.. code:: cpp

    continue_node( const continue_node &src )

Constructs a ``continue_node`` that has the same initial state that ``src`` had after its
construction. It does not copy the current count of ``try_puts`` received, or the current
known number of predecessors. The ``continue_node`` that is constructed has a
reference to the same ``graph`` object as ``src``, has a copy of the initial ``body``
used by ``src``, and only has a non-zero threshold if ``src`` is constructed with a
non-zero threshold.

The new body object is copy-constructed from a copy of the original body provided to ``src``
at its construction.

----------------------------------------------------------------

.. code:: cpp

    bool try_put( const Input &v )

Increments the count of ``try_put()`` calls received. If the incremented count is equal to the
number of known predecessors, performs the ``body`` function object execution. It does not wait
for the execution of the body to complete.

**Returns**: ``true``

----------------------------------------------------------------

.. code:: cpp

    bool try_get( Output &v )

**Returns**: ``false``

Deduction Guides
----------------

.. code:: cpp

    template <typename Body, typename Policy>
    continue_node(graph&, Body, Policy, node_priority_t = no_priority)
        -> continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>, Policy>;

    template <typename Body, typename Policy>
    continue_node(graph&, int, Body, Policy, node_priority_t = no_priority)
        -> continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>, Policy>;

    template <typename Body>
    continue_node(graph&, Body, node_priority_t = no_priority)
        -> continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>, /*default-policy*/>;

    template <typename Body>
    continue_node(graph&, int, Body, node_priority_t = no_priority)
        -> continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>, /*default-policy*/>;

Where:

* ``continue_output_t<Output>`` is an alias to `Output` template argument type. If `Output` specified
  as ``void``, ``continue_output_t<Output>`` is an alias to ``continue_msg`` type.

Example
-------

A set of ``continue_nodes`` forms a :doc:`Dependency Flow Graph <dependency_flow_graph_example>`.
