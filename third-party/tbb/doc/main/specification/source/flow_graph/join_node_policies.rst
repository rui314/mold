.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
join_node Policies
==================
**[flow_graph.join_node_policies]**

``join_node`` supports three buffering policies at its input ports: ``reserving``,
``queueing``, and ``key_matching``.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        struct reserving;
        struct queueing;
        template<typename K, class KHash=tbb_hash_compare<K> > struct key_matching;
        using tag_matching = key_matching<tag_value>;

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi


* ``queueing`` - As each input port is put to, the incoming message is added to
  an unbounded first-in first-out queue in the port. When there is at least one
  message at each input port, the ``join_node`` broadcasts a tuple containing the
  head of each queue to all successors. If at least one successor accepts the
  tuple, the head of each input port's queue is removed; otherwise, the messages
  remain in their respective input port queues.
* ``reserving`` - As each input port is put to, the ``join_node`` marks that an input may be
  available at that port and returns ``false``. When all ports have been marked as
  possibly available, the ``join_node`` tries to reserve a message at
  each port from their known predecessors. If it is unable to reserve a message
  at a port, it unmarks that port, and releases all previously acquired
  reservations. If it is able to reserve a message at all ports, it broadcasts a
  tuple containing these messages to all successors. If at least one successor
  accepts the tuple, the reservations are consumed; otherwise, they are released.
* ``key_matching<typename K, class KHash=tbb_hash_compare<K>>`` - As each input port is put to,
  a user-provided function object is applied to the message to obtain its key. The message is
  then added to a hash table of the input port. When there is a message at each input port for
  a given key, the ``join_node`` removes all matching messages from the input ports,
  constructs a tuple containing the matching messages and attempts to broadcast it to all successors.
  If no successor accepts the tuple, it is saved and will be forwarded on a subsequent ``try_get``.
* ``tag_matching`` - A specialization of ``key_matching`` that accepts keys of type ``tag_value``.
