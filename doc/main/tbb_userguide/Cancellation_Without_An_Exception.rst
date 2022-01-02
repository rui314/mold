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

