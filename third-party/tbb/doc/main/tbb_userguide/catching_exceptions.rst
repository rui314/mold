.. _catching_exceptions:

Catching Exceptions Inside the Node that Throws the Exception
=============================================================


If you catch an exception within the node's body, execution continues
normally, as you might expect. If an exception is thrown but is not
caught before it propagates beyond the node's body, the execution of all
of the graph's nodes are canceled and the exception is rethrown at the
call site of graph::wait_for_all(). Take the graph below as an example:


::


     graph g;


     function_node< int, int > f1( g, 1, []( int i ) {  return i; } );


     function_node< int, int > f2( g, 1, 
         []( const int i ) -> int {
         throw i;
         return i;
     } );


     function_node< int, int > f3( g, 1, []( int i ) {  return i; } );


     make_edge( f1, f2 );
     make_edge( f2, f3 );
     f1.try_put(1);
     f1.try_put(2);
     g.wait_for_all();


In the code above, the second function_node, f2, throws an exception
that is not caught within the body. This will cause the execution of the
graph to be canceled and the exception to be rethrown at the call to
g.wait_for_all(). Since it is not handled there either, the program will
terminate. If desirable, the exception could be caught and handled
within the body:


::


     function_node< int, int > f2( g, 1, 
         []( const int i ) -> int {
             try {
                 throw i;
             } catch (int j) {
                 cout << "Caught " << j << "\n";
             }
             return i;
     } );


If the exception is caught and handled in the body, then there is no
effect on the overall execution of the graph. However, you could choose
instead to catch the exception at the call to wait_for_all:


::


     try {
         g.wait_for_all();
     } catch ( int j ) {
         cout << "Caught " << j << "\n";
     }


In this case, the execution of the graph is canceled. For our example,
this means that the input 1 never reaches f3 and that input 2 never
reaches either f2 or f3.

