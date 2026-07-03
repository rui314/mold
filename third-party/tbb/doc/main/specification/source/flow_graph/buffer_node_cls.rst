.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
buffer_node
===========
**[flow_graph.buffer_node]**

A node that is an unbounded buffer of messages. Messages are forwarded in an arbitrary order.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T>
        class buffer_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit buffer_node( graph &g );
            buffer_node( const buffer_node &src );
            ~buffer_node();

            bool try_put( const T &v );
            bool try_get( T &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the `CopyConstructible` requirements from [copyconstructible] and
  `CopyAssignable` requirements from [copyassignable] ISO C++ Standard sections.

``buffer_node`` is a ``graph_node``, ``receiver<T>``, and ``sender<T>``.

``buffer_node`` has a `buffering` and `single-push` :doc:`properties <forwarding_and_buffering>`.

``buffer_node`` forwards messages in an arbitrary order to a single successor in its successor set.

Member functions
----------------

.. cpp:function:: explicit buffer_node( graph &g )

    Constructs an empty ``buffer_node`` that belongs to the graph ``g``.

.. cpp:function:: explicit buffer_node( const buffer_node &src )

    Constructs an empty ``buffer_node`` that belongs to the same graph ``g`` as ``src``. Any
    intermediate state of ``src``, including its links to predecessors and successors, is not
    copied.

.. cpp:function:: bool try_put( const T &v )

    Adds ``v`` to the set of items managed by the node, and tries forwarding it to a successor.

    **Returns**: ``true``

.. cpp:function:: bool try_get( T &v )

    **Returns**: ``true`` if an item can be removed from the node and assigned to ``v``.
    Returns ``false`` if there is no non-reserved item currently in the node.
