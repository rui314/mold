.. _Exceptions_and_Cancellation:

Exceptions and Cancellation
===========================


|full_name| supports exceptions and
cancellation. When code inside an oneTBB algorithm throws an exception,
the following steps generally occur:


#. The exception is captured. Any further exceptions inside the
   algorithm are ignored.


#. The algorithm is cancelled. Pending iterations are not executed. If
   there is oneTBB parallelism nested inside, the nested parallelism may
   also be cancelled as explained in :ref:`Cancellation_and_Nested_Parallelism`.


#. Once all parts of the algorithm stop, an exception is thrown on the
   thread that invoked the algorithm.


The exception thrown in step 3 might be the original exception, or might
merely be a summary of type ``captured_exception``. The latter usually
occurs on current systems because propagating exceptions between threads
requires support for the C++ ``std::exception_ptr`` functionality. As
compilers evolve to support this functionality, future versions of
oneTBB might throw the original exception. So be sure your code can
catch either type of exception. The following example demonstrates
exception handling.


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
               Data.at(i) += 1;
       }
   };
    

   int main() {
       Data.resize(1000);
       try {
           parallel_for( blocked_range<int>(0, 2000), Update());
       } catch( out_of_range& ex ) {
          cout << "out_of_range: " << ex.what() << endl;
       }
       return 0;
   }


The ``parallel_for`` attempts to iterate over 2000 elements of a vector
with only 1000 elements. Hence the expression ``Data.at(i)`` sometimes
throws an exception ``std::out_of_range`` during execution of the
algorithm. When the exception happens, the algorithm is cancelled and an
exception thrown at the call site to ``parallel_for``.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Cancellation_Without_An_Exception
   ../tbb_userguide/Cancellation_and_Nested_Parallelism
