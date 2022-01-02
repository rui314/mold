.. _Divide_and_Conquer:

Divide and Conquer
==================


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Parallelize a divide and conquer algorithm.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Divide and conquer is widely used in serial algorithms. Common
   examples are quicksort and mergesort.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  Problem can be transformed into subproblems that can be solved
      independently.


   -  Splitting problem or merging solutions is relatively cheap
      compared to cost of solving the subproblems.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   There are several ways to implement divide and conquer in
   |full_name|. The best choice depends upon circumstances.


   -  If division always yields the same number of subproblems, use
      recursion and ``oneapi::tbb::parallel_invoke``.


   -  If the number of subproblems varies, use recursion and
      ``oneapi::tbb::task_group``.


.. container:: section


   .. rubric:: Example
      :class: sectiontitle

   Quicksort is a classic divide-and-conquer algorithm. It divides a
   sorting problem into two subsorts. A simple serial version looks like [1]_.


   ::


      void SerialQuicksort( T* begin, T* end ) {
         if( end-begin>1  ) {
             using namespace std;
             T* mid = partition( begin+1, end, bind2nd(less<T>(),*begin) );
             swap( *begin, mid[-1] );
             SerialQuicksort( begin, mid-1 );
             SerialQuicksort( mid, end );
         }
      }


   The number of subsorts is fixed at two, so ``oneapi::tbb::parallel_invoke``
   provides a simple way to parallelize it. The parallel code is shown
   below:


   ::


      void ParallelQuicksort( T* begin, T* end ) {
         if( end-begin>1 ) {
             using namespace std;
             T* mid = partition( begin+1, end, bind2nd(less<T>(),*begin) );
             swap( *begin, mid[-1] );
             oneapi::tbb::parallel_invoke( [=]{ParallelQuicksort( begin, mid-1 );},
                                   [=]{ParallelQuicksort( mid, end );} );
         }
      }


   Eventually the subsorts become small enough that serial execution is
   more efficient. The following variation, does sorts of less than 500 elements using the earlier serial code.


   ::


      void ParallelQuicksort( T* begin, T* end ) {
         if( end-begin>=500 ) {
             using namespace std;
             T* mid = partition( begin+1, end, bind2nd(less<T>(),*begin) );
             swap( *begin, mid[-1] );
             oneapi::tbb::parallel_invoke( [=]{ParallelQuicksort( begin, mid-1 );},
                                   [=]{ParallelQuicksort( mid, end );} );
         } else {
             SerialQuicksort( begin, end );
         }
      }


   The change is an instance of the Agglomeration pattern.


   The next example considers a problem where there are a variable
   number of subproblems. The problem involves a tree-like description
   of a mechanical assembly. There are two kinds of nodes:


   -  Leaf nodes represent individual parts.


   -  Internal nodes represent groups of parts.


   The problem is to find all nodes that collide with a target node. The
   following code shows a serial solution that walks the tree. It
   records in ``Hits`` any nodes that collide with ``Target``.


   ::


      std::list<Node*> Hits;
      Node* Target;
       

      void SerialFindCollisions( Node& x ) {
         if( x.is_leaf() ) {
             if( x.collides_with( *Target ) )
                 Hits.push_back(&x);
         } else {
             for( Node::const_iterator y=x.begin();y!=x.end(); ++y )
                 SerialFindCollisions(*y);
         }
      } 


   A parallel version is shown below.


   ::


      typedef oneapi::tbb::enumerable_thread_specific<std::list<Node*> > LocalList;
      LocalList LocalHits; 
      Node* Target;    // Target node    
       

      void ParallelWalk( Node& x ) {
         if( x.is_leaf() ) {
             if( x.collides_with( *Target ) )
                 LocalHits.local().push_back(&x);
         } else {
             // Recurse on each child y of x in parallel
             oneapi::tbb::task_group g;
             for( Node::const_iterator y=x.begin(); y!=x.end(); ++y )
                 g.run( [=]{ParallelWalk(*y);} );
             // Wait for recursive calls to complete
             g.wait();
         }
      }
       

      void ParallelFindCollisions( Node& x ) {
         ParallelWalk(x);
         for(LocalList::iterator i=LocalHits.begin();i!=LocalHits.end(); ++i)
             Hits.splice( Hits.end(), *i );
      } 


   The recursive walk is parallelized using class ``task_group`` to do
   recursive calls in parallel.


   There is another significant change because of the parallelism that
   is introduced. Because it would be unsafe to update ``Hits``
   concurrently, the parallel walk uses variable ``LocalHits`` to
   accumulate results. Because it is of type
   ``enumerable_thread_specific``, each thread accumulates its own
   private result. The results are spliced together into Hits after the
   walk completes.


   The results will *not* be in the same order as the original serial
   code.


   If parallel overhead is high, use the agglomeration pattern. For
   example, use the serial walk for subtrees under a certain threshold.


.. [1] Production quality quicksort implementations typically
   use more sophisticated pivot selection, explicit stacks instead of
   recursion, and some other sorting algorithm for small subsorts. The
   simple algorithm is used here to focus on exposition of the parallel
   pattern.

