.. _communicate_with_nodes:

Communication Between Graphs
============================


All graph nodes require a reference to a graph object as one of the
arguments to their constructor. It is only safe to construct edges
between nodes that are part of the same graph. An edge expresses the
topology of your graph to the runtime library. Connecting two nodes in
different graphs can make it difficult to reason about whole graph
operations, such as calls to graph::wait_for_all and exception handling.
To optimize performance, the library may make calls to a node's
predecessor or successor at times that are unexpected by the user.


If two graphs must communicate, do NOT create an edge between them, but
instead use explicit calls to try_put. This will prevent the runtime
library from making any assumptions about the relationship of the two
nodes, and therefore make it easier to reason about events that cross
the graph boundaries. However, it may still be difficult to reason about
whole graph operations. For example, consider the graphs below:


::


       graph g;
       function_node< int, int > n1( g, 1, [](int i) -> int { 
           cout << "n1\n"; 
           spin_for(i); 
           return i; 
       } );
       function_node< int, int > n2( g, 1, [](int i) -> int { 
           cout << "n2\n"; 
           spin_for(i); 
           return i; 
       } );
       make_edge( n1, n2 );


       graph g2;
       function_node< int, int > m1( g2, 1, [](int i) -> int { 
           cout << "m1\n"; 
           spin_for(i); 
           return i; 
       } );
       function_node< int, int > m2( g2, 1, [&](int i) -> int { 
           cout << "m2\n"; 
           spin_for(i); 
           n1.try_put(i); 
           return i; 
       } );
       make_edge( m1, m2 );


       m1.try_put( 1 );


       // The following call returns immediately:
       g.wait_for_all();
       // The following call returns after m1 & m2
       g2.wait_for_all();


       // we reach here before n1 & n2 are finished
       // even though wait_for_all was called on both graphs


In the example above, m1.try_put(1) sends a message to node m1, which
runs its body and then sends a message to node m2. Next, node m2 runs
its body and sends a message to n1 using an explicit try_put. In turn,
n1 runs its body and sends a message to n2. The runtime library does not
consider m2 to be a predecessor of n1 since no edge exists.


If you want to wait until all of the tasks spawned by these graphs are
done, you need to call the function wait_for_all on both graphs.
However, because there is cross-graph communication, the order of the
calls is important. In the (incorrect) code segment above, the first
call to g.wait_for_all() returns immediately because there are no tasks
yet active in g; the only tasks that have been spawned by then belong to
g2. The call to g2.wait_for_all returns after both m1 and m2 are done,
since they belong to g2; the call does not however wait for n1 and n2,
since they belong to g. The end of this code segment is therefore
reached before n1 and n2 are done.


If the calls to wait_for_all are swapped, the code works as expected:


::


       g2.wait_for_all();
       g.wait_for_all();


       // all tasks are done


While it is not too difficult to reason about how these two very small
graphs interact, the interaction of two larger graphs, perhaps with
cycles, will be more difficult to understand. Therefore, communication
between nodes in different graphs should be done with caution.

