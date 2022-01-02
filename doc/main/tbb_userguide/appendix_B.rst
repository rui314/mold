.. _appendix_B:

Appendix B Mixing With Other Threading Packages
===============================================


|full_name| can be mixed with other
threading packages. No special effort is required to use any part of
oneTBB with other threading packages.


Here is an example that parallelizes an outer loop with OpenMP and an
inner loop with oneTBB.


::


   int M, N;
    

   struct InnerBody {
       ...
   };
    

   void TBB_NestedInOpenMP() {
   #pragma omp parallel
       {
   #pragma omp for
           for( int i=0; i<M; ++ ) {
               parallel_for( blocked_range<int>(0,N,10), InnerBody(i) );
           }
       }
   }


The details of ``InnerBody`` are omitted for brevity. The
``#pragma omp parallel`` causes the OpenMP to create a team of threads,
and each thread executes the block statement associated with the pragma.
The ``#pragma omp for`` indicates that the compiler should use the
previously created thread team to execute the loop in parallel.


Here is the same example written using POSIX\* Threads.


::


   int M, N;
    

   struct InnerBody {
       ...
   };
    

   void* OuterLoopIteration( void* args ) {
       int i = (int)args;
       parallel_for( blocked_range<int>(0,N,10), InnerBody(i) );
   }
    

   void TBB_NestedInPThreads() {
       std::vector<pthread_t> id( M );
       // Create thread for each outer loop iteration
       for( int i=0; i<M; ++i )
           pthread_create( &id[i], NULL, OuterLoopIteration, NULL );
       // Wait for outer loop threads to finish
       for( int i=0; i<M; ++i )
           pthread_join( &id[i], NULL );
   } 

