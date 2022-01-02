.. _create_token_based_system:

Create a Token-Based System
===========================


A more flexible solution to limit the number of messages in a flow graph
is to use tokens. In a token-based system, a limited number of tokens
are available in the graph and a message will not be allowed to enter
the graph until it can be paired with an available token. When a message
is retired from the graph, its token is released, and can be paired with
a new message that will then be allowed to enter.


The ``oneapi::tbb::parallel_pipeline`` algorithm relies on a token-based system. In
the flow graph interface, there is no explicit support for tokens, but
``join_node``s can be used to create an analogous system. A ``join_node`` has
two template arguments, the tuple that describes the types of its inputs
and a buffer policy:


::


   template<typename OutputTuple, graph_buffer_policy JP = queueing>
   class join_node;


The buffer policy can be one of the following:


-  ``queueing``. This type of policy causes inputs to be matched
   first-in-first-out; that is, the inputs are joined together to form a
   tuple in the order they are received.
-  ``tag_matching``. This type of policy joins inputs together that have
   matching tags.
-  ``reserving``. This type of policy causes the ``join_node`` to do no
   internally buffering, but instead to consume inputs only when it can
   first reserve an input on each port from an upstream source. If it
   can reserve an input at each port, it gets those inputs and joins
   those together to form an output tuple.


A token-based system can be created by using reserving join_nodes.


In the example below, there is an ``input_node`` that generates ``M`` big
objects and a ``buffer_node`` that is pre-filled with three tokens. The
``token_t`` can be anything, for example it could be ``typedef int token_t;``.
The ``input_node`` and ``buffer_node`` are connected to a reserving ``join_node``.
The ``input_node`` will only generate an input when one is pulled from it
by the reserving ``join_node``, and the reserving ``join_node`` will only pull
the input from the ``input_node`` when it knows there is also an item to
pull from the ``buffer_node``.


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

     join_node< tuple_t, reserving > j(g);


     buffer_node< token_t > b(g);


     function_node< tuple_t, token_t > f( g, unlimited, 
       []( const tuple_t &t ) -> token_t {
           spin_for(1);
        cout << get<1>(t) << "\n";
           delete get<0>(t);
        return get<1>(t);
     } );


     make_edge( s, input_port<0>(j) );
     make_edge( b, input_port<1>(j) );
     make_edge( j, f );
     make_edge( f, b );


     b.try_put( 1 );
     b.try_put( 2 );
     b.try_put( 3 );


     g.wait_for_all();


In the above code, you can see that the ``function_node`` returns the token
back to the ``buffer_node``. This cycle in the flow graph allows the token
to be recycled and paired with another input from the ``input_node``. So
like in the previous sections, there will be at most four big objects in
the graph. There could be three big objects in the ``function_node`` and one
buffered in the ``input_node``, awaiting a token to be paired with.


Since there is no specific ``token_t`` defined for the flow graph, you can
use any type for a token, including objects or pointers to arrays.
Therefore, unlike in the example above, the ``token_t`` doesn't need to be a
dummy type; it could for example be a buffer or other object that is
essential to the computation. We could, for example, modify the example
above to use the big objects themselves as the tokens, removing the need
to repeatedly allocate and deallocate them, and essentially create a
free list of big objects using a cycle back to the ``buffer_node``.


Also, in our example above, the ``buffer_node`` was prefilled by a fixed
number of explicit calls to ``try_put``, but there are other options. For
example, an ``input_node`` could be attached to the input of the
``buffer_node``, and it could generate the tokens. In addition, our
``function_node`` could be replaced by a ``multifunction_node`` that can
optionally put 0 or more outputs to each of its output ports. Using a
``multifunction_node``, you can choose to recycle or not recycle a token, or
even generate more tokens, thereby increasing or decreasing the allowed
concurrency in the graph.


A token based system is therefore very flexible. You are free to declare
the token to be of any type and to inject or remove tokens from the
system as it is executing, thereby having dynamic control of the allowed
concurrency in the system. Since you can pair the token with an input at
the source, this approach enables you to limit resource consumption
across the entire graph.

