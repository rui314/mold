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

The graph object does not own the nodes associated with it. You need to make sure that the graph object's lifetime is longer than the lifetimes of all nodes added to the graph and any activity associated with the graph. 

.. tip:: Call ``wait_for_all`` on a graph object before destroying it to make sure all activities are complete. 

 Even when using smart pointers, be aware of the order of destruction for nodes and the graph to make sure that nodes are not deleted before the graph.


