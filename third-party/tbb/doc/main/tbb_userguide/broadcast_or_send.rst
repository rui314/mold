.. _broadcast_or_send:

Sending to One or Multiple Successors
=====================================


An important characteristic of the predefined nodes is whether they push
their output to a single successor or broadcast to all successors. The
following predefined nodes push messages to a single successor:


-  ``buffer_node``
-  ``queue_node``
-  ``priority_queue_node``
-  ``sequencer_node``


Other nodes push messages to all successors that will accept them.


The nodes that push to only a single successor are all buffer nodes.
Their purpose is to hold messages temporarily, until they are consumed
downstream. Consider the example below:


::


   void use_buffer_and_two_nodes() {
     graph g;


     function_node< int, int, rejecting > f1( g, 1, []( int i ) -> int {
       spin_for(0.1);
       cout << "f1 consuming " << i << "\n";
       return i; 
     } );


     function_node< int, int, rejecting > f2( g, 1, []( int i ) -> int {
       spin_for(0.2);
       cout << "f2 consuming " << i << "\n";
       return i; 
     } );


     priority_queue_node< int > q(g);


     make_edge( q, f1 );
     make_edge( q, f2 );
     for ( int i = 10; i > 0; --i ) {
       q.try_put( i );
     }
     g.wait_for_all();
   }


First, function_nodes by default queue up the messages they receive at
their input. To make a ``priority_queue_node`` work properly with a
``function_node``, the example above constructs its ``function_nodes`` with its
buffer policy set to rejecting. So, ``f1`` and ``f2`` do not internally buffer
incoming messages, but instead rely on upstream buffering in the
``priority_queue_node``.


In the above example, each message buffered by the ``priority_queue_node``
is sent to either ``f1`` or ``f2``, but not both.


Let's consider the alternative behavior; that is; what if the
``priority_queue_node`` broadcasts to all successors. What if some, but not
all, nodes accept a message? Should the message be buffered until all
nodes accept it, or be only delivered to the accepting subset? If the
node continues to buffer the message, should it eventually deliver the
messages in the same order to all nodes or in the current priority order
at the time the node accepts the next message? For example, assume a
``priority_queue_node`` only contains "9" when a successor node, ``f1``, accepts
"9" but another successor node, ``f2``, rejects it. Later a value "100"
arrives and ``f2`` is available to accept messages. Should ``f2`` receive "9"
next or "100", which has a higher priority? In any case, trying to
ensure that all successors receive each message creates a garbage
collection problem and complicates reasoning. Therefore, these buffering
nodes push each message to only one successor. And, you can use this
characteristic to create useful graph structures such as the one shown
in the graph above, where each message will be processed in priority
order, by either ``f1`` or ``f2``.


But what if you really do want both ``f1`` and ``f2`` to receive all of the
values, and in priority order? You can easily create this behavior by
creating one ``priority_queue_node`` for each ``function_node``, and pushing
each value to both queues through a broadcast_node, as shown below:


::


     graph g;


     function_node< int, int, rejecting > f1( g, 1, []( int i ) -> int {
       spin_for(0.1);
       cout << "f1 consuming " << i << "\n";
       return i; 
     } );


     function_node< int, int, rejecting > f2( g, 1, []( int i ) -> int {
       spin_for(0.2);
       cout << "f2 consuming " << i << "\n";
       return i; 
     } );


     priority_queue_node< int > q1(g);
     priority_queue_node< int > q2(g);
     broadcast_node< int > b(g);


     make_edge( b, q1 );
     make_edge( b, q2 );
     make_edge( q1, f1 );
     make_edge( q2, f2 );
     for ( int i = 10; i > 0; --i ) {
       b.try_put( i );
     }
     g.wait_for_all();


So, when connecting a node in your graph to multiple successors, be sure
to understand whether the output will broadcast to all of the
successors, or just a single successor.

