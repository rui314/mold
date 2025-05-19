.. _use_input_node:

Using input_node
=================


By default, an ``input_node`` is constructed in the inactive state:


::


   template< typename Body > input_node( graph &g, Body body, bool is_active=true )


To activate an inactive ``input_node``, you call the node's function
activate:


::


       input_node< int > src( g, src_body(10), false );
       // use it in calls to make_edge…
       src.activate();


All ``input_node`` objects are constructed in the inactive state and usually
activated after the entire flow graph is constructed.


For example, you can use the code in :ref:`Data_Flow_Graph`. In that implementation,
the ``input_node`` is constructed in the inactive state and activated after
all other edges are made:


::


         make_edge( squarer, summer );
         make_edge( cuber, summer );
         input_node< int > src( g, src_body(10), false );
         make_edge( src, squarer );
         make_edge( src, cuber );
         src.activate();
         g.wait_for_all();


In this example, if the ``input_node`` was toggled to the active state at the beginning,
it might send a message to squarer immediately after the edge to
squarer is connected. Later, when the edge to cuber is connected, cuber
will receive all future messages, but may have already missed some.


In general it is safest to create your ``input_node`` objects in the inactive
state and then activate them after the whole graph is constructed.
However, this approach serializes graph construction and graph
execution.


Some graphs can be constructed safely with ``input_node`` active, allowing
the overlap of construction and execution. If your graph is a directed
acyclic graph (DAG), and each ``input_node`` has only one successor, you
can activate your ``input_node`` just after their construction if you construct the
edges in reverse topological order; that is, make the edges at the
largest depth in the tree first, and work back to the shallowest edges.
For example, if src is an ``input_node`` and ``func1`` and ``func2`` are both
function nodes, the following graph would not drop messages, even though
src is activated just after its construction:


::


       const int limit = 10;
       int count = 0;
       graph g;
       oneapi::tbb::flow::graph g;
       oneapi::tbb::flow::input_node<int> src( g, [&]( oneapi::tbb::flow_control &fc ) -> int {
         if ( count < limit ) {
           return ++count;
         }
         fc.stop();
         return {};
       });
       src.activate();

       oneapi::tbb::flow::function_node<int,int> func1( g, 1, []( int i ) -> int {
         std::cout << i << "\n";
         return i;
       } );
       oneapi::tbb::flow::function_node<int,int> func2( g, 1, []( int i ) -> int {
         std::cout << i << "\n";
         return i;
       } );


       make_edge( func1, func2 );
       make_edge( src, func1 );


       g.wait_for_all();


The above code is safe because the edge from ``func1`` to ``func2`` is made
before the edge from src to ``func1``. If the edge from src to func1 were
made first, ``func1`` might generate a message before ``func2`` is attached to
it; that message would be dropped. Also, src has only a single
successor. If src had more than one successor, the successor that is
attached first might receive messages that do not reach the successors
that are attached after it.

