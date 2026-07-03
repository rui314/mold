.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
overwrite_node
==============
**[flow_graph.overwrite_node]**

A node that is a buffer of a single item that can be overwritten.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<typename T>
        class overwrite_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit overwrite_node( graph &g );
            overwrite_node( const overwrite_node &src );
            ~overwrite_node();

            bool try_put( const T &v );
            bool try_get( T &v );

            bool is_valid( );
            void clear( );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the `DefaultConstructible` requirements from [defaultconstructible] and
  `CopyAssignable` requirements from [copyassignable] ISO C++ Standard sections.

This type of node buffers a single item of type ``T``. The value is initially invalid. Gets from the node are
non-destructive.

``overwrite_node`` is a ``graph_node``, ``receiver<T>`` and ``sender<T>``.

``overwrite_node`` has a `buffering` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

``overwrite_node`` allows overwriting its single item buffer.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::overwrite_node
	       
.. cpp:function:: explicit overwrite_node( graph &g )

    Constructs an object of type ``overwrite_node`` that belongs to the graph ``g`` with an invalid
    internal buffer item.

.. cpp:function:: overwrite_node( const overwrite_node &src )

    Constructs an object of type ``overwrite_node`` that belongs to the graph ``g`` with an invalid
    internal buffer item. The buffered value and list of successors and predecessors are not copied from ``src``.

.. cpp:function:: ~overwrite_node( )

    Destroys the overwrite_node.

.. cpp:function:: bool try_put( const T &v )

    Stores ``v`` in the internal single item buffer and calls ``try_put(v)`` on all successors.

    **Returns**: ``true``

.. cpp:function:: bool try_get( T &v )

    If the internal buffer is valid, assigns the value to ``v``.

    **Returns**:``true`` if ``v`` is assigned to; ``false``, otherwise.

.. cpp:function:: bool is_valid( )

    **Returns**: ``true`` if the buffer holds a valid value; ``false``, otherwise.

.. cpp:function:: void clear( )

    Invalidates the value held in the buffer.

Examples
--------

The example demonstrates ``overwrite_node`` as a single-value storage that
might be updated. Data can be accessed with direct ``try_get()`` call.

.. include:: ./examples/overwrite_node_cls.cpp
   :code: cpp

``overwrite_node`` supports reserving ``join_node`` as its successor. See the example in :doc:`the
example section of write_once_node <write_once_node_cls>`.
