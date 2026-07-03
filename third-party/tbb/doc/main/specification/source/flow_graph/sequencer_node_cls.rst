.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
sequencer_node
==============
**[flow_graph.sequencer_node]**

A node that forwards messages in a sequence order.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T >
        class sequencer_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            template< typename Sequencer >
            sequencer_node( graph &g, const Sequencer &s );
            sequencer_node( const sequencer_node &src );

            bool try_put( const T &v );
            bool try_get( output_type &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the `CopyConstructible` requirements from [copyconstructible] and
  `CopyAssignable`  requirements from [copyassignable] ISO C++ Standard sections.
* The type ``Sequencer`` must meet the :doc:`Sequencer requirements <../named_requirements/flow_graph/sequencer>`.
  Since C++17, ``Sequencer`` may also be a pointer to a const member function in ``T`` that returns ``size_t`` or
  a pointer to a data member in ``T`` of type ``size_t``.
  If ``Sequencer`` instance throws an exception, behavior is undefined.

``sequencer_node`` forwards messages in a sequence order to a single successor in its successor set.

``sequencer_node`` is a ``graph_node``, ``receiver<T>`` and ``sender<T>``.

Each item that passes through a ``sequencer_node`` is ordered by its sequencer order
number. These sequence order numbers range from 0 to the largest integer representable by the
``std::size_t`` type. Sequencer order number of an item is determined by passing the item to a
user-provided ``Sequencer`` function object.

.. note::

    The ``sequencer_node`` rejects duplicate sequencer numbers.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::sequencer_node

.. cpp:function:: template<typename Sequencer> \
                  sequencer_node( graph& g, const Sequencer &s )

    Constructs an empty ``sequencer_node`` that belongs to the graph ``g`` and uses ``s`` to compute
    sequence numbers for items.

.. cpp:function:: sequencer_node( const sequencer_node &src )

    Constructs an empty ``sequencer_node`` that belongs to the same graph ``g`` as ``src`` and
    uses a copy of the ``Sequencer s`` used to construct ``src``. The list of predecessors,
    the list of successors, and the messages inside are not copied.

    .. caution::

        The new sequencer object is copy-constructed from a copy of the original sequencer object
        provided to ``src`` at its construction. Changes made to member variables in
        the ``src`` object do not affect the sequencer of the new ``sequencer_node.``

.. cpp:function:: bool try_put( const T &v )

    Adds ``v`` to the ``sequencer_node`` and tries forwarding the next item in the sequence to a
    successor.

    **Returns**: ``true``

.. cpp:function:: bool try_get( T &v )

    **Returns**: ``true`` if the next item in the sequence is available in the ``sequencer_node``.
    If so, it is removed from the node and assigned to ``v``. Returns ``false`` if the next item
    in sequencer order is not available or if the node is reserved.


Deduction Guides
----------------

.. code:: cpp

    template <typename Body>
    sequencer_node(graph&, Body) -> input_node<std::decay_t<input_t<Body>>>;

Where:

* ``input_t`` is an alias to ``Body`` input argument type.

Example
-------

The example demonstrates ordering capabilities of the ``sequencer_node``.
While being processed in parallel, the data is passed to the successor node in the exact same order
it was read.

.. include:: ./examples/sequencer_node_cls.cpp
    :code: cpp
