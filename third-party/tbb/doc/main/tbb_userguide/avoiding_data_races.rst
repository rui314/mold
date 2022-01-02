.. _avoiding_data_races:

Avoiding Data Races
===================


The edges in a flow graph make explicit the dependence relationships
that you want the library to enforce. Similarly, the concurrency limits
on ``function_node`` and ``multifunction_node`` objects limit the maximum number
of concurrent invocations that the runtime library will allow. These are
the limits that are enforced by the library; the library does not
automatically protect you from data races. You must explicitly prevent
data races by using these mechanisms.


For example, the follow code has a data race because there is nothing to
prevent concurrent accesses to the global count object referenced by
node f:


::


     graph g;
     int src_count = 1;
     int global_sum = 0;
     int limit = 100000;

     input_node< int > src( g, [&]( oneapi::tbb::flow_control& fc ) -> int {
       if ( src_count <= limit ) {
         return src_count++;
       } else {
         fc.stop();
         return int();
       }
     } );
     src.activate();

     function_node< int, int > f( g, unlimited, [&]( int i ) -> int {
       global_sum += i;  // data race on global_sum
       return i; 
     } );


     make_edge( src, f );
     g.wait_for_all();


     cout << "global sum = " << global_sum 
          << " and closed form = " << limit*(limit+1)/2 << "\n";


If you run the above example, it will likely calculate a global sum that
is a bit smaller than the expected solution due to the data race. The
data race could be avoided in this simple example by changing the
allowed concurrency in ``f`` from unlimited to 1, forcing each value to be
processed sequentially by ``f``. You may also note that the ``input_node`` also
updates a global value, ``src_count``. However, since an ``input_node`` always
executes serially, there is no race possible.
