.. _use_nested_flow_graphs:

Use Nested Flow Graphs
======================


In addition to nesting algorithms within a flow graph node, it is also
possible to nest flow graphs. For example, below there is a graph ``g`` with
two nodes, ``a`` and ``b``. When node ``a`` receives a message, it constructs and
executes an inner dependence graph. When node ``b`` receives a message, it
constructs and executes an inner data flow graph:


::


     graph g;
       function_node< int, int > a( g, unlimited, []( int i ) -> int {
           graph h;
           node_t n1( h, [=]( msg_t ) { cout << "n1: " << i << "\n"; } );
           node_t n2( h, [=]( msg_t ) { cout << "n2: " << i << "\n"; } );
           node_t n3( h, [=]( msg_t ) { cout << "n3: " << i << "\n"; } );
           node_t n4( h, [=]( msg_t ) { cout << "n4: " << i << "\n"; } );
           make_edge( n1, n2 );
           make_edge( n1, n3 );
           make_edge( n2, n4 );
           make_edge( n3, n4 );
           n1.try_put(continue_msg());
           h.wait_for_all();
           return i;
       } );
       function_node< int, int > b( g, unlimited, []( int i ) -> int {
           graph h;
           function_node< int, int > m1( h, unlimited, []( int j ) -> int {
               cout << "m1: " << j << "\n";
               return j;
           } );
           function_node< int, int > m2( h, unlimited, []( int j ) -> int {
               cout << "m2: " << j << "\n";
               return j;
           } );
           function_node< int, int > m3( h, unlimited, []( int j ) -> int {
               cout << "m3: " << j << "\n";
               return j;
           } );
           function_node< int, int > m4( h, unlimited, []( int j ) -> int {
               cout << "m4: " << j << "\n";
               return j;
           } );
           make_edge( m1, m2 );
           make_edge( m1, m3 );
           make_edge( m2, m4 );
           make_edge( m3, m4 );
           m1.try_put(i);
           h.wait_for_all();
           return i;
       } );
       make_edge( a, b );
       for ( int i = 0; i < 3; ++i ) {
           a.try_put(i);
       }
       g.wait_for_all();


If the nested graph remains unchanged in structure between invocations
of the node, it is redundant to construct it each time. Reconstructing
the graph only adds overhead to the execution. You can modify the
example above, for example, to have node ``b`` reuse a graph that is
persistent across its invocations:


::


     graph h;
       function_node< int, int > m1( h, unlimited, []( int j ) -> int {
           cout << "m1: " << j << "\n";
           return j;
       } );
       function_node< int, int > m2( h, unlimited, []( int j ) -> int {
           cout << "m2: " << j << "\n";
           return j;
       } );
       function_node< int, int > m3( h, unlimited, []( int j ) -> int {
           cout << "m3: " << j << "\n";
           return j;
       } );
       function_node< int, int > m4( h, unlimited, []( int j ) -> int {
           cout << "m4: " << j << "\n";
           return j;
       } );
       make_edge( m1, m2 );
       make_edge( m1, m3 );
       make_edge( m2, m4 );
       make_edge( m3, m4 );


       graph g;
       function_node< int, int > a( g, unlimited, []( int i ) -> int {
           graph h;
           node_t n1( h, [=]( msg_t ) { cout << "n1: " << i << "\n"; } );
           node_t n2( h, [=]( msg_t ) { cout << "n2: " << i << "\n"; } );
           node_t n3( h, [=]( msg_t ) { cout << "n3: " << i << "\n"; } );
           node_t n4( h, [=]( msg_t ) { cout << "n4: " << i << "\n"; } );
           make_edge( n1, n2 );
           make_edge( n1, n3 );
           make_edge( n2, n4 );
           make_edge( n3, n4 );
           n1.try_put(continue_msg());
           h.wait_for_all();
           return i;
       } );
       function_node< int, int > b( g, unlimited, [&]( int i ) -> int {
           m1.try_put(i);
           h.wait_for_all(); // optional since h is not destroyed
           return i;
       } );
       make_edge( a, b );
       for ( int i = 0; i < 3; ++i ) {
           a.try_put(i);
       }
       g.wait_for_all();


It is only necessary to call ``h.wait_for_all()`` at the end of each
invocation of ``b``'s body in our modified code, if you wish for this ``b``'s
body to block until the inner graph is done. In the first implementation
of ``b``, it was necessary to call ``h.wait_for_all`` at the end of each
invocation since the graph was destroyed at the end of the scope. So it
would be valid in the body of ``b`` above to call ``m1.try_put(i)`` and then
return without waiting for ``h`` to become idle.

