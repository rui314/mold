.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
input_node
==========
**[flow_graph.input_node]**

A node that generates messages by invoking the user-provided functor and broadcasts the
result to all of its successors.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template < typename Output >
        class input_node : public graph_node, public sender<Output> {
        public:
            template< typename Body >
            input_node( graph &g, Body body );
            input_node( const input_node &src );
            ~input_node();

            void activate();
            bool try_get( Output &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Output`` type must meet the `DefaultConstructible` requirements from [defaultconstructible], `CopyConstructible` requirements from [copyconstructible] and
  `CopyAssignable`  requirements from [copyassignable] ISO C++ Standard sections.
* The type ``Body`` must meet the :doc:`InputNodeBody requirements <../named_requirements/flow_graph/input_node_body>`.

This node can have no predecessors. It executes a user-provided ``body`` function object to
generate messages that are broadcast to all successors. It is a serial node and never calls
its ``body`` concurrently. This node can buffer a single item.  If no successor accepts an
item that it has generated, the message is buffered and provided to successors
before a new item is generated.

``input_node`` is a ``graph_node`` and ``sender<Output>``.

``input_node`` has a `buffering` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

An ``input_node`` continues to invoke ``body`` and broadcast messages until the ``body``
toggles ``fc.stop()`` or it has no valid successors. A message may be generated and then rejected
by all successors. In this case, the message is buffered and will be the next message sent once a
successor is added to the node or ``try_get`` is called. Calls to ``try_get`` return a
message from the buffer, or invoke ``body`` to attempt to generate a new message.
A call to ``body`` is made only when the buffer is empty.

The body object passed to an ``input_node`` is copied. Updates to member variables do
not affect the original object used to construct the node. If the state held within a body object
must be inspected from outside of the node, the :doc:`copy_body function <copy_body_func>` can be
used to obtain an updated copy.

Member functions
----------------

.. cpp:function:: template< typename Body > input_node( graph &g, Body body )

    Constructs an ``input_node`` that invokes ``body``. By default,
    the node is created in an inactive state, which means that messages are not generated until a call to ``activate`` is made.

.. cpp:function:: input_node( const input_node &src )

    Constructs an ``input_node`` that has the same initial state that
    ``src`` had when it was constructed. The ``input_node`` that is
    constructed has a reference to the same ``graph`` object as
    ``src``, has a copy of the initial body used by ``src,`` and
    has the same initial active state as ``src``. The successors of
    ``src`` are not copied.

    The new body object is copy-constructed from a copy of the original
    body provided to ``src`` at its construction. Changes
    made to member variables in ``src`` body after the construction
    of ``src`` do not affect the body of the new ``input_node.``

.. cpp:function:: void activate()

    Sets the ``input_node`` to the active state, which enables messages generation.

.. cpp:function:: bool try_get( Output &v )

    Copies the message from the buffer to ``v`` if available, or, if the node is
    in active state, invokes ``body`` to attempt to generate a new message that
    will be copied into ``v``. 

    **Returns:** ``true`` if a message is copied to ``v``;  ``false``, otherwise.

Deduction Guides
----------------

.. code:: cpp

    template <typename Body>
    input_node(graph&, Body) -> input_node<std::decay_t<input_t<Body>>>;

Where:

* ``input_t`` is an alias to ``Body`` input argument type.
