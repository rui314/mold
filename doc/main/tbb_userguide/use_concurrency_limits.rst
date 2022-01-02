.. _use_concurrency_limits:

Use Concurrency Limits
======================


To control the number of instances of a single node, you can use the
concurrency limit on the node. To cause it to reject messages after it
reaches its concurrency limit, you construct it as a "rejecting" node.


A function node is constructed with one or more template arguments. The
third argument controls the buffer policy used by the node, and is by
default queueing. With a queueing policy, a ``function_node`` that has
reached its concurrency limit still accepts incoming messages, but
buffers them internally. If the policy is set to rejecting the node will
instead reject the incoming messages.


::


   template < typename Input,
              typename Output = continue_msg,
              graph_buffer_policy = queueing >
   class function_node;


For example, you can control the number of big objects in flight in a
graph by placing a rejecting function_node downstream of an ``input_node``,
as is done below:


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

     function_node< big_object *, continue_msg, rejecting > f( g, 3, 
         []( big_object *v ) -> continue_msg {
         spin_for(1);
            delete v;
         return continue_msg();
     } );


     make_edge( s, f );
     g.wait_for_all();


The ``function_node`` will operate on at most three big objects
concurrently. The node's concurrency threshold that limits the node to
three concurrent invocations. When the ``function_node`` is running three
instances concurrently, it will start rejecting incoming messages from
the ``input_node``, causing the ``input_node`` to buffer its last created
object and temporarily stop invoking its body object. Whenever the
``function_node`` drops below its concurrency limit, it will pull new
messages from the ``input_node``. At most four big objects will exist
simultaneously, three in the ``function_node`` and one buffered in the
``input_node``.

