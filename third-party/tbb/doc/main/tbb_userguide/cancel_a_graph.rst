.. _cancel_a_graph:

Cancel a Graph Explicitly
=========================


To cancel a graph execution without an exception, you can create the
graph using an explicit task_group_context, and then call
cancel_group_execution() on that object. This is done in the example
below:


::


     task_group_context t;
     graph g(t);


     function_node< int, int > f1( g, 1, []( int i ) {  return i; } );


     function_node< int, int > f2( g, 1, 
         []( const int i ) -> int {
             cout << "Begin " << i << "\n";
             spin_for(0.2);
             cout << "End " << i << "\n";
             return i;
     } );


     function_node< int, int > f3( g, 1, []( int i ) {  return i; } );


     make_edge( f1, f2 );
     make_edge( f2, f3 );
     f1.try_put(1);
     f1.try_put(2);
     spin_for(0.1);
     t.cancel_group_execution();
     g.wait_for_all();


When a graph execution is canceled, any node that has already started to
execute will execute to completion, but any node that has not started to
execute will not start. So in the example above, f2 will print both the
Begin and End message for input 1, but will not receive the input 2.


You can also get the task_group_context that a node belongs to from
within the node body and use it to cancel the execution of the graph it
belongs to:


::


     graph g;


     function_node< int, int > f1( g, 1, []( int i ) {  return i; } );


     function_node< int, int > f2( g, 1, 
         []( const int i ) -> int {
          cout << "Begin " << i << "\n";
          spin_for(0.2);
             cout << "End " << i << "\n";
             task::self().group()->cancel_group_execution();
             return i;
     } );


     function_node< int, int > f3( g, 1, []( int i ) {  return i; } );


     make_edge( f1, f2 );
     make_edge( f2, f3 );
     f1.try_put(1);
     f1.try_put(2);
     g.wait_for_all();


You can get the task_group_context from a node's body even if the graph
was not explicitly passed one at construction time.

