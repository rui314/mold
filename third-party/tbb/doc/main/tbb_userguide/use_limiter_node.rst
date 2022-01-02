.. _use_limiter_node:

Using limiter_node
==================


One way to limit resource consumption is to use a limiter_node to set a
limit on the number of messages that can flow through a given point in
your graph. The constructor for a limiter node takes two arguments:


::


   limiter_node( graph &g, size_t threshold )


The first argument is a reference to the graph it belongs to. The second
argument sets the maximum number of items that should be allowed to pass
through before the node starts rejecting incoming messages.


A limiter_node maintains an internal count of the messages that it has
allowed to pass. When a message leaves the controlled part of the graph,
a message can be sent to the decrement port on the ``limiter_node`` to
decrement the count, allowing additional messages to pass through. In
the example below, an ``input_node`` will generate ``M`` big objects. But the
user wants to allow at most three big objects to reach the ``function_node``
at a time, and to prevent the ``input_node`` from generating all ``M`` big
objects at once.


::


     graph g;


     int src_count = 0;
     int number_of_objects = 0;
     int max_objects = 3;


     input_node< big_object * > s( g, [&]( oneapi::tbb::flow_control& fc ) -> big_object* {
         if ( src_count < M ) {
           big_object* v = new big_object();
           ++src_count;
           return v;
         } else {
           fc.stop();
           return nullptr;
         }
     } );
     s.activate();

     limiter_node< big_object * > l( g, max_objects );


     function_node< big_object *, continue_msg > f( g, unlimited, 
       []( big_object *v ) -> continue_msg {
         spin_for(1);
         delete v;
         return continue_msg();
     } );




     make_edge( l, f );
     make_edge( f, l.decrement );
     make_edge( s, l );
     g.wait_for_all();


The example above prevents the ``input_node`` from generating all ``M`` big
objects at once. The ``limiter_node`` has a threshold of 3, and will
therefore start rejecting incoming messages after its internal count
reaches 3. When the ``input_node`` sees its message rejected, it stops
calling its body object and temporarily buffers the last generated
value. The ``function_node`` has its output, a ``continue_msg``, sent to the
decrement port of the ``limiter_node``. So, after it completes executing,
the ``limiter_node`` internal count is decremented. When the internal count
drops below the threshold, messages begin flowing from the ``input_node``
again. So in this example, at most four big objects exist at a time, the
three that have passed through the ``limiter_node`` and the one that is
buffered in the ``input_node``.

