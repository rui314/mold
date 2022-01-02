.. _avoid_dynamic_node_removal:

Avoid Dynamic Node Removal
==========================


These are the basic guidelines regarding nodes and edges:


-  Avoid dynamic node removal


-  Adding edges and nodes is supported


-  Removing edges is supported


It is possible to add new nodes and edges and to remove old edges from a
flow graph as nodes are actively processing messages in the graph.
However, removing nodes is discouraged. Destroying a graph or any of its
nodes while there are messages being processed in the graph can lead to
premature deletion of memory that will be later touched by tasks in the
graph causing program failure. Removal of nodes when the graph is not
idle may lead to intermittent failures and hard to find failures, so it
should be avoided.

