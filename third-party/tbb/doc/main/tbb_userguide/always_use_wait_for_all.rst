.. _always_use_wait_for_all:

Always Use wait_for_all()
=========================


One of the most common mistakes made in flow graph programming is to
forget to call ``wait_for_all``. The function ``graph::wait_for_all`` blocks
until all tasks spawned by the graph are complete. This is not only
useful when you want to wait until the computation is done, but it is
necessary to call ``wait_for_all`` before destroying the graph, or any of
its nodes. For example, the following function will lead to a program
failure:


::


   void no_wait_for_all() {
       graph g;
       function_node< int, int > f( g, 1, []( int i ) -> int {
           return spin_for(i);
       } );
       f.try_put(1);


       // program will fail when f and g are destroyed at the
       // end of the scope, since the body of f is not complete
   }


In the function above, the graph g and its node f are destroyed at the
end of the function's scope. However, the task spawned to execute f's
body is still in flight. When the task completes, it will look for any
successors connected to its node, but by then both the graph and the
node have been deleted out from underneath it. Placing a
``g.wait_for_all()`` at the end of the function prevents the premature
destruction of the graph and node.


If you use a flow graph and see mysterious behavior, check first to see
that you have called ``wait_for_all``.

