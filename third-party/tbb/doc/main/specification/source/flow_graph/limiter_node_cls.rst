.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
limiter_node
============
**[flow_graph.limiter_node]**

A node that counts and limits the number of messages that pass through it.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T, typename DecrementType=continue_msg >
        class limiter_node : public graph_node, public receiver<T>, public sender<T> {
        public:
            limiter_node( graph &g, size_t threshold );
            limiter_node( const limiter_node &src );

            receiver<DecrementType>& decrementer();

            bool try_put( const T &v );
            bool try_get( T &v );
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* ``T`` type must meet the `DefaultConstructible` requirements from
  [defaultconstructible] ISO C++ Standard section.
* The ``DecrementType`` type must be an integral type or ``continue_msg``.

``limiter_node`` is a ``graph_node``, ``receiver<T>``, and ``sender<T>``

``limiter_node`` has a `discarding` and `broadcast-push` :doc:`properties <forwarding_and_buffering>`.

This node does not accept new messages once the user-specified ``threshold`` is
reached. The internal counter of broadcasts is adjusted through use of
the *decrementer*, a ``receiver`` object embedded into the node that
can be obtained by calling the ``decrementer`` method. The counter values are truncated to be
inside the [0, ``threshold``] interval.

The template parameter ``DecrementType`` specifies the type of the message that
can be sent to the decrementer. This template parameter is defined to
``continue_msg`` by default. If an integral type is specified, positive values sent
to the decrementer determine the value by which the internal counter of broadcasts
will be decreased, while negative values determine the value by which the internal
counter of broadcasts will be increased.

If ``continue_msg`` is used as an argument for the ``DecrementType`` template
parameter, the ``decrementer``'s port of the ``limiter_node`` also acquires the
behavior of the ``continue_node``. This behavior requires the number of messages sent to it
to be equal to the number of connected predecessors before decrementing the
internal counter of broadcasts by one.

When ``try_put`` call on the decrementer results in
the new value of the counter of broadcasts to be less than the
``threshold``, the ``limiter_node`` tries to get a message from one
of its known predecessors and forward that message to all its
successors. If it cannot obtain a message from a predecessor, it decrements the counter of broadcasts.

Member functions
----------------

.. namespace:: oneapi::tbb::flow::limiter_node
	       
.. cpp:function:: limiter_node( graph &g, size_t threshold )

    Constructs a ``limiter_node`` that allows up to ``threshold`` items
    to pass through before rejecting ``try_put``'s.

.. cpp:function:: limiter_node( const limiter_node &src )

    Constructs a ``limiter_node`` that has the same initial state that
    ``src`` had at its construction. The new ``limiter_node``
    belongs to the same graph ``g`` as ``src``, has the same
    ``threshold``. The list of predecessors, the list of successors,
    and the current count of broadcasts are not copied from ``src``.

.. cpp:function:: receiver<DecrementType>& decrementer()

    Obtains a reference to the embedded ``receiver`` object that is used for the
    internal counter adjustments.

.. cpp:function:: bool try_put( const T &v )

    If the broadcast count is below the threshold, ``v`` is broadcast
    to all successors.

    **Returns**: ``true`` if ``v`` is broadcast; ``false`` if ``v``
    is not broadcast because the threshold has been reached.

.. cpp:function:: bool try_get( T &v )

    **Returns**: ``false``.
