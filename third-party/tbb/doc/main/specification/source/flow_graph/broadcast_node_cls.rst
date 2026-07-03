.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
broadcast_node
==============
**[flow_graph.broadcast_node]**

A node that broadcasts incoming messages to all of its successors.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>
    
    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T >
        class broadcast_node :
        public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit broadcast_node( graph &g );
            broadcast_node( const broadcast_node &src );

            bool try_put( const T &v );
            bool try_get( T &v );
        };

    } // namespace flow
    } // namespace tbb
    } //namespace oneapi

``broadcast_node`` is a ``graph_node``, ``receiver<T>``, and ``sender<T>``.

``broadcast_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

All messages are forwarded immediately to all successors.

Member functions
----------------

.. cpp:function:: explicit broadcast_node( graph &g )

  Constructs an object of type ``broadcast_node`` that belongs to the
  graph ``g``.

.. cpp:function:: broadcast_node( const broadcast_node &src )

  Constructs an object of type ``broadcast_node`` that belongs to the
  same graph ``g`` as ``src``. The list of predecessors and the list of
  successors are not copied.

.. cpp:function:: bool try_put( const input_type &v )

  Broadcasts ``v`` to all successors.

  **Returns**: always returns ``true``, even if it was unable to
  successfully forward the message to any of its successors.

.. cpp:function:: bool try_get( output_type &v )

  **Returns**: ``false``.
