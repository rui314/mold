.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
priority_queue_node
===================
**[flow_graph.priority_queue_node]**

A class template that forwards messages in a priority order.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T, typename Compare = std::less<T>>
        class priority_queue_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            explicit priority_queue_node( graph &g );
            priority_queue_node( const priority_queue_node &src );
            ~priority_queue_node();

            bool try_put( const T &v );
            bool try_get( T &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the `CopyConstructible` requirements from [copyconstructible] and
  `CopyAssignable` requirements from [copyassignable] ISO C++ Standard sections.
* The type ``Compare`` must meet the `Compare` type requirements from [alg.sorting] ISO C++
  Standard section. If ``Compare`` instance throws an exception, then behavior is undefined.

The next message to be forwarded has the largest priority as determined by the ``Compare`` template argument.

``priority_queue_node`` is a ``graph_node``, ``receiver<T>``, and ``sender<T>``.

``priority_queue_node`` has a `buffering` and `single-push` :doc:`properties <forwarding_and_buffering>`.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::priority_node_queue
	       
.. cpp:function:: explicit priority_queue_node( graph &g )

    Constructs an empty ``priority_queue_node`` that belongs to the  graph ``g``.

.. cpp:function:: priority_queue_node( const priority_queue_node &src )

    Constructs an empty ``priority_queue_node`` that belongs to the same graph ``g`` as ``src``. Any
    intermediate state of ``src``, including its links to predecessors and successors, is not
    copied.

.. cpp:function:: bool try_put( const T &v )

    Adds ``v`` to the ``priority_queue_node`` and tries forwarding to a successor the item with the
    largest priority among all of the items that were added to the node and have not been yet
    forwarded to successors.

    **Returns**: ``true``

.. cpp:function:: bool try_get( T &v )

    **Returns**: ``true`` if a message is available in the node and the node is not currently reserved.
    Otherwise, returns ``false``. If the node returns ``true``, the message with the largest
    priority is copied to ``v``.

Example
-------

Usage scenario is similar to :doc:`sequencer_node <sequencer_node_cls>` except that the
``priority_queue_node`` provides local order, passing the message with highest priority of all
stored at the moment, while ``sequencer_node`` enforces global order and does not allow a
"smaller priority" message to pass through before all preceding messages.
