.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
write_once_node
===============
**[flow_graph.write_once_node]**

A node that is a buffer of a single item that cannot be overwritten.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T >
        class write_once_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit write_once_node( graph &g );
            write_once_node( const write_once_node &src );
            ~write_once_node();

            bool try_put( const T &v );
            bool try_get( T &v );

            bool is_valid( );
            void clear( );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``T`` type must meet the `DefaultConstructible` requirements from [defaultconstructible] and
  `CopyAssignable` requirements from [copyassignable] ISO C++ Standard sections.

This type of node buffers a single item of type ``T``. The value is initially invalid. Gets from the node
are non-destructive.

``write_once_node`` is a ``graph_node``, ``receiver<T>`` and ``sender<T>``.

``write_once_node`` has a `buffering` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

``write_once_node`` does not allow overwriting its single item buffer.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::write_once_mode

.. cpp:function:: explicit write_once_node( graph &g )

    Constructs an object of type ``write_once_node`` that belongs to the graph ``g``, with an
    invalid internal buffer item.

.. cpp:function:: write_once_node( const write_once_node &src )

    Constructs an object of type ``write_once_node`` with an invalid internal buffer item. The
    buffered value and list of successors is not copied from ``src``.

.. cpp:function:: ~write_once_node( )

    Destroys the ``write_once_node``.

.. cpp:function:: bool try_put( const T &v )

    Stores ``v`` in the internal single item buffer if it does not contain a valid value already.
    If a new value is set, the node broadcast it to all successors.

    **Returns**: ``true`` for the first time after construction or a call to ``clear()``; ``false``,
    otherwise.

.. cpp:function:: bool try_get( T &v )

    If the internal buffer is valid, assigns the value to ``v``.

    **Returns**: ``true`` if ``v`` is assigned to; ``false``, otherwise.

.. cpp:function:: bool is_valid( )

  **Returns**: ``true`` if the buffer holds a valid value; ``false``, otherwise.

.. cpp:function:: void clear( )

    Invalidates the value held in the buffer.

Example
-------

Usage scenario is similar to :doc:`overwrite_node <overwrite_node_cls>` but an internal buffer can
be updated only after ``clear()`` call. The following example shows the possibility to connect the
node to a reserving ``join_node``, avoiding direct calls to the ``try_get()`` method from the body
of the successor node.

.. include:: ./examples/write_once_node_cls.cpp
   :code: cpp
