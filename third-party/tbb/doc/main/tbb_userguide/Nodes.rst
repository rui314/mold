.. _Nodes:

Flow Graph Basics: Nodes
========================


A node is a class that inherits from ``oneapi::tbb::flow::graph_node`` and also
typically inherits from ``oneapi::tbb::flow::sender<T>``, ``oneapi::tbb::flow::receiver<T>``, or
both. A node performs some operation, usually on an incoming message and
may generate zero or more output messages. Some nodes require more than
one input message or generate more than one output message.


While it is possible to define your own node types by inheriting from
graph_node, sender and receiver, it is more typical that predefined node
types are used to construct a graph.


A ``function_node`` is a predefined type available in ``flow_graph.h`` and
represents a simple function with one input and one output. The
constructor for a ``function_node`` takes three arguments:


::


   template< typename Body> function_node(graph &g, size_t concurrency, Body body)


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1

      * -  Parameter 
        -  Description 
      * -  Body 
        -     Type of the body object.     
      * -  g 
        -     The graph the node belongs to.     
      * -  concurrency 
        -     The concurrency limit for the node. You can use the    concurrency limit to control how many invocations of the node are   allowed to proceed concurrently, from 1 (serial) to an unlimited   number.    
      * -  body 
        -     User defined function object, or lambda expression, that    is applied to the incoming message to generate the outgoing message.      




Below is code for creating a simple graph that contains a single
function_node. In this example, a node n is constructed that belongs to
graph g, and has a second argument of 1, which allows at most 1
invocation of the node to occur concurrently. The body is a lambda
expression that prints each value v that it receives, spins for v
seconds, prints the value again, and then returns v unmodified. The code
for the function spin_for is not provided.


::


       graph g;
       function_node< int, int > n( g, 1, []( int v ) -> int { 
           cout << v;
           spin_for( v );
           cout << v;
           return v;
       } );


After the node is constructed in the example above, you can pass
messages to it, either by connecting it to other nodes using edges or by
invoking its function try_put. Using edges is described in the next
section.


::


       n.try_put( 1 );
       n.try_put( 2 );
       n.try_put( 3 );


You can then wait for the messages to be processed by calling
``wait_for_all`` on the graph object:


::


       g.wait_for_all(); 


In the above example code, the function_node n was created with a
concurrency limit of 1. When it receives the message sequence 1, 2 and
3, the node n will spawn a task to apply the body to the first input, 1.
When that task is complete, it will then spawn another task to apply the
body to 2. And likewise, the node will wait for that task to complete
before spawning a third task to apply the body to 3. The calls to
try_put do not block until a task is spawned; if a node cannot
immediately spawn a task to process the message, the message will be
buffered in the node. When it is legal, based on concurrency limits, a
task will be spawned to process the next buffered message.


In the above graph, each message is processed sequentially. If however,
you construct the node with a different concurrency limit, parallelism
can be achieved:


::


       function_node< int, int > n( g, oneapi::tbb::flow::unlimited, []( int v ) -> int { 
           cout << v;
           spin_for( v );
           cout << v;
           return v;
       } );


You can use unlimited as the concurrency limit to instruct the library
to spawn a task as soon as a message arrives, regardless of how many
other tasks have been spawned. You can also use any specific value, such
as 4 or 8, to limit concurrency to at most 4 or 8, respectively. It is
important to remember that spawning a task does not mean creating a
thread. So while a graph may spawn many tasks, only the number of
threads available in the library's thread pool will be used to execute
these tasks.


Suppose you use unlimited in the function_node constructor instead and
call try_put on the node:


::


       n.try_put( 1 );
       n.try_put( 2 );
       n.try_put( 3 );
       g.wait_for_all(); 


The library spawns three tasks, each one applying n's lambda expression
to one of the messages. If you have a sufficient number of threads
available on your system, then all three invocations of the body will
occur in parallel. If however, you have only one thread in the system,
they execute sequentially.

