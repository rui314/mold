.. _work_isolation:

Work Isolation
==============


.. container:: section


   In |full_name|, a thread waiting for a
   group of tasks to complete might execute other available tasks. In
   particular, when a parallel construct calls another parallel
   construct, a thread can obtain a task from the outer-level construct
   while waiting for completion of the inner-level one.


   In the following example with two ``parallel_for`` calls, the call to
   the second (nested) parallel loop blocks execution of the first
   (outer) loop iteration:


   ::


      // The first parallel loop.
      oneapi::tbb::parallel_for( 0, N1, []( int i ) { 
          // The second parallel loop.
          oneapi::tbb::parallel_for( 0, N2, []( int j ) { /* Some work */ } );
      } );


   The blocked thread is allowed to take tasks belonging to the first
   parallel loop. As a result, two or more iterations of the outer loop
   might be simultaneously assigned to the same thread. In other words,
   in oneTBB execution of functions constituting a parallel construct is
   *unsequenced* even within a single thread. In most cases, this
   behavior is harmless or even beneficial because it does not restrict
   parallelism available for the thread.


   However, in some cases such unsequenced execution may result in
   errors. For example, a thread-local variable might unexpectedly
   change its value after a nested parallel construct:


   ::


      oneapi::tbb::enumerable_thread_specific<int> ets;
      oneapi::tbb::parallel_for( 0, N1, [&ets]( int i ) {
          // Set a thread specific value
          ets.local() = i;
          oneapi::tbb::parallel_for( 0, N2, []( int j ) { /* Some work */ } );
          // While executing the above parallel_for, the thread might have run iterations
          // of the outer parallel_for, and so might have changed the thread specific value.
          assert( ets.local()==i ); // The assertion may fail!
      } );


   In other scenarios, the described behavior might lead to deadlocks
   and other issues. In these cases, a stronger guarantee of execution
   being sequenced within a thread is desired. For that, oneTBB provides
   ways to *isolate* execution of a parallel construct, for its tasks to
   not interfere with other simultaneously running tasks.


   One of these ways is to execute the inner level loop in a separate
   ``task_arena``:


   ::


      oneapi::tbb::enumerable_thread_specific<int> ets;
      oneapi::tbb::task_arena nested;
      oneapi::tbb::parallel_for( 0, N1, [&]( int i ) {
          // Set a thread specific value
          ets.local() = i;
          nested.execute( []{
              // Run the inner parallel_for in a separate arena to prevent the thread
              // from taking tasks of the outer parallel_for.
              oneapi::tbb::parallel_for( 0, N2, []( int j ) { /* Some work */ } );
          } );
          assert( ets.local()==i ); // Valid assertion
      } );


   However, using a separate arena for work isolation is not always
   convenient, and might have noticeable overheads. To address these
   shortcomings, oneTBB provides ``this_task_arena::isolate`` function
   which runs a user-provided functor in isolation by restricting the
   calling thread to process only tasks scheduled in the scope of the
   functor (also called the isolation region).


   When entered a task waiting call or a blocking parallel construct
   inside an isolated region, a thread can only execute tasks spawned
   within the region and their child tasks spawned by other threads. The
   thread is prohibited from executing any outer level tasks or tasks
   belonging to other isolated regions.


   The isolation region imposes restrictions only upon the thread that
   called it. Other threads running in the same task arena have no
   restrictions on task selection unless isolated by a distinct call to
   ``this_task_arena::isolate``.


   The following example demonstrates the use of
   ``this_task_arena::isolate`` to ensure that a thread-local variable
   is not changed unexpectedly during the call to a nested parallel
   construct.


   ::


      #include "oneapi/tbb/task_arena.h"
      #include "oneapi/tbb/parallel_for.h"
      #include "oneapi/tbb/enumerable_thread_specific.h"
      #include <cassert>


      int main() {
          const int N1 = 1000, N2 = 1000;
          oneapi::tbb::enumerable_thread_specific<int> ets;
          oneapi::tbb::parallel_for( 0, N1, [&ets]( int i ) {
              // Set a thread specific value
              ets.local() = i;
              // Run the second parallel loop in an isolated region to prevent the current thread
              // from taking tasks related to the outer parallel loop.
              oneapi::tbb::this_task_arena::isolate( []{
                  oneapi::tbb::parallel_for( 0, N2, []( int j ) { /* Some work */ } );
              } );
              assert( ets.local()==i ); // Valid assertion
          } );
          return 0;
      }

