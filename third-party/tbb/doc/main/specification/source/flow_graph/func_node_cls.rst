.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
function_node
=============
**[flow_graph.function_node]**

A node that executes a user-provided body on incoming messages.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template < typename Input, typename Output = continue_msg, typename Policy = /*implementation-defined*/ >
        class function_node : public graph_node, public receiver<Input>, public sender<Output> {
        public:
            template<typename Body>
            function_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                           node_priority_t priority = no_priority );
            template<typename Body>
            function_node( graph &g, size_t concurrency, Body body,
                           node_priority_t priority = no_priority );
            ~fuction_node();

            function_node( const function_node &src );

            bool try_put( const Input &v );
            bool try_get( Output &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Input`` type must meet the `DefaultConstructible` requirements from
  [defaultconstructible] and the `CopyConstructible` requirements from
  [copyconstructible] ISO C++ Standard sections.
* The ``Output`` type must meet the `CopyConstructible` requirements from
  [copyconstructible] ISO C++ Standard section.
* The type ``Policy`` may be specified as :doc:`lightweight, queueing and rejecting policies<functional_node_policies>` or defaulted.
* The type ``Body`` must meet the :doc:`FunctionNodeBody requirements <../named_requirements/flow_graph/function_node_body>`.
  Since C++17, ``Body`` may also be a pointer to a const member function in ``Input`` that returns ``Output`` or
  a pointer to a data member in ``Input`` of type ``Output``.

``function_node`` has a user-settable concurrency limit. It can be set to one of :doc:`predefined values <predefined_concurrency_limits>`.
The user can also provide a value of type ``std::size_t`` to limit concurrency to a value between 1 and :doc:`tbb::flow::unlimited <predefined_concurrency_limits>`.

Messages that cannot be immediately processed due to concurrency limits are handled according to
the `Policy` template argument.

``function_node`` is a ``graph_node``, ``receiver<Input>``, and ``sender<Output>``.

``function_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

The body object passed to a ``function_node`` is copied. Updates to member variables do
not affect the original object used to construct the node. If the state held within a body object must be
inspected from outside of the node, the :doc:`copy_body <copy_body_func>` function can be used to obtain an updated copy.

Member functions
----------------

.. code:: cpp

    template<typename Body>
    function_node( graph &g, size_t concurrency, Body body,
                   node_priority_t priority = no_priority );

Constructs a ``function_node`` that invokes a copy of ``body``. Most of ``concurrency`` calls
to ``body`` can be made concurrently.

Use this function to specify :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    template<typename Body>
    function_node( graph &g, size_t concurrency, Body body, Policy /*unspecified*/ = Policy(),
                   node_priority_t priority = no_priority );

Constructs a ``function_node`` that invokes a copy of ``body``. Most of ``concurrency`` calls
to ``body`` can be made concurrently.

Use this function to specify :doc:`policy<functional_node_policies>` and :doc:`node priority<node_priorities>`.

----------------------------------------------------------------

.. code:: cpp

    function_node( const function_node &src )

Constructs a ``function_node`` that has the same initial state that ``src`` had when it was
constructed. The ``function_node`` that is constructed has a reference to the same ``graph``
object as ``src``, has a copy of the initial body used by ``src``, and has the same
concurrency threshold as ``src``. The predecessors and successors of ``src`` are not copied.

The new body object is copy-constructed from a copy of the original body provided to ``src`` at
its construction. Changes made to member variables in ``src``'s body after the
construction of ``src`` do not affect the body of the new ``function_node.``

----------------------------------------------------------------

.. code:: cpp

    bool try_put( const Input &v )

If the concurrency limit allows, executes the user-provided body on the incoming message ``v``.
Otherwise, depending on the policy of the node, either queues the incoming message ``v`` or rejects
it.

**Returns:** ``true`` if the input was accepted; and ``false``, otherwise.

----------------------------------------------------------------

.. code:: cpp

    bool try_get( Output &v )

**Returns:** ``false``

Deduction Guides
----------------

.. code:: cpp

    template <typename Body, typename Policy>
    function_node(graph&, size_t, Body, Policy, node_priority_t = no_priority)
        ->function_node<std::decay_t<input_t<Body>>, output_t<Body>, Policy>;

    template <typename Body>
    function_node(graph&, size_t, Body, node_priority_t = no_priority)
        ->function_node<std::decay_t<input_t<Body>>, output_t<Body>, /*default-policy*/>;

Where:

* ``input_t`` is an alias to ``Body`` input argument type.
* ``output_t`` is an alias to ``Body`` return type.

Example
-------

:doc:`Data Flow Graph example <message_flow_graph_example>` illustrates how ``function_node`` performs
computation on input data and passes the result to successors.
