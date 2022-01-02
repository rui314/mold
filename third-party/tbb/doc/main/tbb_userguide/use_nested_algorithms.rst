.. _use_nested_algorithms:

Use Nested Algorithms to Increase Scalability
=============================================


One powerful way to increase the scalability of a flow graph is to nest
other parallel algorithms inside of node bodies. Doing so, you can use a
flow graph as a coordination language, expressing the most
coarse-grained parallelism at the level of the graph, with finer grained
parallelism nested within.


In the example below, five nodes are created: an ``input_node``,
``matrix_source``, that reads a sequence of matrices from a file, two
``function_nodes``, ``n1`` and ``n2``, that receive these matrices and generate two
new matrices by applying a function to each element, and two final
``function_nodes``, ``n1_sink`` and ``n2_sink``, that process these resulting
matrices. The ``matrix_source`` is connected to both ``n1`` and ``n2``. The node ``n1``
is connected to ``n1_sink``, and ``n2`` is connected to ``n2_sink``. In the lambda
expressions for ``n1`` and ``n2``, a ``parallel_for`` is used to apply the functions
to the elements of the matrix in parallel. The functions
``read_next_matrix``, ``f1``, ``f2``, ``consume_f1`` and ``consume_f2`` are not provided
below.


::


       graph g;
       input_node< double * > matrix_source( g, [&]( oneapi::tbb::flow_control &fc ) -> double* {
         double *a = read_next_matrix();
         if ( a ) {
           return a;
         } else {
           fc.stop();
           return nullptr;
         }
       } );
       function_node< double *, double * > n1( g, unlimited, [&]( double *a ) -> double * {
         double *b = new double[N];
         parallel_for( 0, N, [&](int i) {
           b[i] = f1(a[i]);
         } );
         return b;
       } );
       function_node< double *, double * > n2( g, unlimited, [&]( double *a ) -> double * {
         double *b = new double[N];
         parallel_for( 0, N, [&](int i) {
           b[i] = f2(a[i]);
         } );
         return b;
       } );
       function_node< double *, double * > n1_sink( g, unlimited, 
         []( double *b ) -> double * {
           return consume_f1(b);
       } );
       function_node< double *, double * > n2_sink( g, unlimited, 
         []( double *b ) -> double * {
           return consume_f2(b);
       } );
       make_edge( matrix_source, n1 );
       make_edge( matrix_source, n2 );
       make_edge( n1, n1_sink );
       make_edge( n2, n2_sink );
       matrix_source.activate();
       g.wait_for_all();

