.. _destroy_graphs_outside_main_thread:

Destroying Graphs That Run Outside the Main Thread
==================================================

Make sure to enqueue a task to wait for and destroy graphs that run outside the main thread.

You may not always want to block the main application thread by calling
``wait_for_all()``. However, it is safest to call ``wait_for_all`` on a graph
before destroying it. A common solution is to enqueue a task to build
and wait for the graph to complete. For example, assume you really do
not want to call a ``wait_for_all`` in the example from :ref:`always_use_wait_for_all`,
Instead you can enqueue a task that creates the graph and waits for it:


::


   class background_task {
   public:
     void operator()() {
       graph g;
       function_node< int, int > f( g, 1, []( int i ) -> int {
         return spin_for(i);
       } );
       f.try_put(1);
       g.wait_for_all();
     }
   };


   void no_wait_for_all_enqueue() {
     task_arena a;
     a.enqueue(background_task());
     // do other things without waiting…
   }


In the code snippet above, the enqueued task executes at some point, but
it's not clear when. If you need to use the results of the enqueued
task, or even ensure that it completes before the program ends, you will
need to use some mechanism to signal from the enqueued task that the
graph is complete.

