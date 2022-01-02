.. _Edges:

Flow Graph Basics: Edges
========================


Most applications contain multiple nodes with edges connecting them to
each other. In the flow graph interface, edges are directed channels
over which messages are passed. They are created by calling the function
``make_edge( p, s )`` with two arguments: ``p``, the predecessor, and ``s``, the
successor. You can modify the example used in the **Nodes** topic to
include a second node that squares the value it receives before printing
it and then connect that to the first node with an edge.


::


       graph g;
       function_node< int, int > n( g, unlimited, []( int v ) -> int { 
           cout << v;
           spin_for( v );
           cout << v;
           return v;
       } );
       function_node< int, int > m( g, 1, []( int v ) -> int {
           v *= v;
           cout << v;
           spin_for( v );
           cout << v;
           return v;
       } );
       make_edge( n, m );
       n.try_put( 1 );
       n.try_put( 2 );
       n.try_put( 3 );
       g.wait_for_all();


Now there are two ``function_node``s, ``n`` and ``m``. The call to ``make_edge`` creates
an edge from ``n`` to ``m``. The node n is created with unlimited concurrency,
while ``m`` has a concurrency limit of 1. The invocations of ``n`` can all
proceed in parallel, while the invocations of ``m`` will be serialized.
Because there is an edge from ``n`` to ``m``, each value ``v``, returned by ``n``, will
be automatically passed to node ``m`` by the runtime library.

