.. _Flow_Graph_Buffering_in_Nodes:

Flow Graph Basics: Buffering and Forwarding
===========================================


|full_name| flow graph nodes use messages
to communicate data and to enforce dependencies. If a node passes a
message successfully to any successor, no further action is taken with
the message by that node. As noted in the section on Single-push vs.
Broadcast-push, a message may be passed to one or to multiple
successors, depending on the type of the node, how many successors are
connected to the node, and whether the message is pushed or pulled.


There are times when a node cannot successfully push a message to any
successor. In this case what happens to the message depends on the type
of the node. The two possibilities are:


-  The node stores the message to be forwarded later.
-  The node discards the message.


If a node discards messages that are not forwarded, and this behavior is
not desired, the node should be connected to a buffering node that does
store messages that cannot be pushed.


If a message has been stored by a node, there are two ways it can be
passed to another node:


-  A successor to the node can pull the message using ``try_get()`` or
   ``try_reserve()``.
-  A successor can be connected using ``make_edge()``.


If a ``try_get()`` successfully forwards a message, it is removed from
the node that stored it. If a node is connected using ``make_edge`` the
node will attempt to push a stored message to the new successor.

