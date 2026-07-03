.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
queue_node
==========
**[flow_graph.queue_node]**

A node that forwards messages in a first-in first-out (FIFO) order.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template <typename T >
        class queue_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit queue_node( graph &g );
            queue_node( const queue_node &src );
            ~queue_node();

            bool try_put( const T &v );
            bool try_get( T &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the `CopyConstructible` requirements from [copyconstructible] and
  `CopyAssignable`  requirements from [copyassignable] ISO C++ Standard sections.

``queue_node`` forwards messages in a FIFO order to a single successor in
its successor set.

``queue_node`` is a ``graph_node``, ``receiver`` and ``sender``.

``queue_node`` has a `buffering` and `single-push` :doc:`properties <forwarding_and_buffering>`.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::queue_node

.. cpp:function:: explicit queue_node( graph &g )

    Constructs an empty ``queue_node`` that belongs to the  graph ``g``.

.. cpp:function:: queue_node( const queue_node &src )

    Constructs an empty ``queue_node`` that belongs to the same graph ``g`` as ``src``. Any
    intermediate state of ``src``, including its links to predecessors and successors, is not copied.

.. cpp:function:: bool try_put( const T &v )

    Adds ``v`` to the set of items managed by the node, and tries forwarding the least recently
    added item to a successor.

    **Returns**: ``true``.

.. cpp:function:: bool try_get( T &v )

    **Returns**: ``true`` if an item can be taken from the node and assigned to ``v``. Returns
    ``false`` if there is no item currently in the ``queue_node`` or if the node is reserved.

Example
-------

Usage scenario is similar to :doc:`buffer_node <buffer_node_cls>` except that messages are passed
in first-in first-out (FIFO) order.
