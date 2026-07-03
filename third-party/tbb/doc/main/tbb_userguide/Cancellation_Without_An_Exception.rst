.. _Cancellation_Without_An_Exception:

Cancellation Without An Exception
=================================


To cancel an algorithm but not throw an exception, use the expression ``current_context()->cancel_group_execution()``.
The part ``current_context()`` references the ``task_group_context*`` of the currently executing task if any on the current thread.
Calling ``cancel_group_execution()`` cancels all tasks in its ``task_group_context``, which is explained in more detail in :ref:`Cancellation_and_Nested_Parallelism`.
The method returns ``true`` if it actually causes cancellation, ``false`` if the ``task_group_context`` was already cancelled.

The example below shows how to use ``current_context()->cancel_group_execution()``.

::

   #include "oneapi/tbb.h"

   #include <vector>
   #include <iostream>
    
   using namespace oneapi::tbb;
   using namespace std;
    
   vector<int> Data;
    
   struct Update {
       void operator()( const blocked_range<int>& r ) const {
           for( int i=r.begin(); i!=r.end(); ++i )
               if( i<Data.size() ) {
                   ++Data[i];
               } else {
                   // Cancel related tasks.
                   if( current_context()->cancel_group_execution() )
                       cout << "Index " << i << " caused cancellation\n";
                   return;
               }
       }
   };
    

   int main() {
       Data.resize(1000);
       parallel_for( blocked_range<int>(0, 2000), Update());
       return 0;
   }

task_group Cancellation Example
===============================

The interface to a ``task_group`` provides shortcuts to access its asoociated ``task_group_context``.
The function ``oneapi::tbb::task_group::cancel()`` cancels the ``task_group_context`` associated
with the ``task_group`` instance. And the free function
``oneapi::tbb::is_current_task_group_canceling()`` returns ``true`` if the innermost ``task_group``
executing on the calling thread is cancelling its tasks.

Here is an example of how to use task cancellation with ``oneapi::tbb::task_group``. This code
uses ``struct TreeNode`` and the function ``sequential_tree_search`` that are described in
:ref:`creating_tasks_with_parallel_invoke`.

The function ``parallel_tree_search_cancellable_impl`` cancels the ``task_group`` when a result
is found.

.. literalinclude:: ./examples/task_examples.cpp
    :language: c++
    :start-after: /*begin_parallel_search_cancellation*/
    :end-before: /*end_parallel_search_cancellation*/

The call to ``tg.cancel()`` cancels the tasks in the ``task_group`` that have been submitted but
have not started to execute. The task scheduler will not execute these tasks. Those tasks that
have already started will not be interrupted but they can query the cancellation status of the
``task_group`` by calling ``oneapi::tbb::is_current_task_group_canceling()`` and exit early if
they detect cancellation.