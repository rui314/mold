.. _Graph_Object:

Flow Graph Basics: Graph Object
===============================


Conceptually a flow graph is a collection of nodes and edges. Each node
belongs to exactly one graph and edges are made only between nodes in
the same graph. In the flow graph interface, a graph object represents
this collection of nodes and edges, and is used for invoking whole graph
operations such as waiting for all tasks related to the graph to
complete, resetting the state of all nodes in the graph, and canceling
the execution of all nodes in the graph.


The code below creates a graph object and then waits for all tasks
spawned by the graph to complete. The call to ``wait_for_all`` in this
example returns immediately since this is a trivial graph with no nodes
or edges, and therefore no tasks are spawned.


::


   graph g;
   g.wait_for_all();

