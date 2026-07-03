.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

========================
Forwarding and Buffering
========================
**[flow_graph.forwarding_and_buffering]**

Forwarding
----------

In a ``flow::graph``, nodes that forward messages to successors have
one of two possible forwarding policies, which are a property of the node:

* **broadcast-push** - the message will be pushed to as many successors as will accept
  the message. If no successor accepts the message, the fate of the message depends on the
  output buffering policy of the node.
* **single-push** - if the message is accepted by a successor, no further push of that
  message will occur. If a successor rejects the message, the next successor in the set is tried.
  This continues until a successor accepts the message, or all successors have been attempted.
  If no successor accepts the message, it will be retained for a possible future resend.
  Message that is successfully transferred to a successor is removed from the node.

Buffering
----------

There are two policies for handling a message that cannot be pushed to any successor:

* **buffering** - if no successor accepts a message, it is stored so subsequent node
  processing can use it. Nodes that buffer outputs have "yes" in the "try_get()?" column
  below.
* **discarding** - if no successor accepts a message, it is discarded and has no
  further effect on graph execution. Nodes that discard outputs have "no" in the
  "try_get()?" column below.

The following table lists the policies of each node:

.. table:: Buffering and Forwarding properties summary

   ================================= ========== ==============================
   Node                              try_get()? Forwarding
   ================================= ========== ==============================
   **Functional Nodes**
   --------------------------------- ---------- ------------------------------
   ``input_node``                    yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   ``function_node<rejecting>``      no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``function_node<queueing>``       no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``continue_node``                 no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``multifunction_node<rejecting>`` no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``multifunction_node<queueing>``  no         broadcast-push
   --------------------------------- ---------- ------------------------------
   **Buffering Nodes**
   --------------------------------- ---------- ------------------------------
   ``buffer_node``                   yes        single-push
   --------------------------------- ---------- ------------------------------
   ``priority_queue_node``           yes        single-push
   --------------------------------- ---------- ------------------------------
   ``queue_node``                    yes        single-push
   --------------------------------- ---------- ------------------------------
   ``sequencer_node``                yes        single-push
   --------------------------------- ---------- ------------------------------
   ``overwrite_node``                yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   ``write_once_node``               yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   **Split/Join Nodes**
   --------------------------------- ---------- ------------------------------
   ``join_node<queueing>``           yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   ``join_node<reserving>``          yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   ``join_node<tag_matching>``       yes        broadcast-push
   --------------------------------- ---------- ------------------------------
   ``split_node``                    no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``indexer_node``                  no         broadcast-push
   --------------------------------- ---------- ------------------------------
   **Other Nodes**
   --------------------------------- ---------- ------------------------------
   ``broadcast_node``                no         broadcast-push
   --------------------------------- ---------- ------------------------------
   ``limiter_node``                  no         broadcast-push
   ================================= ========== ==============================
