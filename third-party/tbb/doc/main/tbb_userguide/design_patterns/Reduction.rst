.. _Reduction:

Reduction
=========


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Perform an associative reduction operation across a data set.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Many serial algorithms sweep over a set of items to collect summary
   information.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   The summary can be expressed as an associative operation over the
   data set, or at least is close enough to associative that
   reassociation does not matter.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Two solutions exist in |full_name|.
   The choice on which to use depends upon several considerations:


   -  Is the operation commutative as well as associative?


   -  Are instances of the reduction type expensive to construct and
      destroy. For example, a floating point number is inexpensive to
      construct. A sparse floating-point matrix might be very expensive
      to construct.


   Use ``oneapi::tbb::parallel_reduce`` when the objects are inexpensive to
   construct. It works even if the reduction operation is not
   commutative.


   Use ``oneapi::tbb::parallel_for`` and ``oneapi::tbb::combinable`` if the reduction
   operation is commutative and instances of the type are expensive.


   If the operation is not precisely associative but a precisely
   deterministic result is required, use recursive reduction and
   parallelize it using ``oneapi::tbb::parallel_invoke``.


.. container:: section


   .. rubric:: Examples
      :class: sectiontitle

   The examples presented here illustrate the various solutions and some
   tradeoffs.


   The first example uses ``oneapi::tbb::parallel_reduce`` to do a + reduction
   over sequence of type ``T``. The sequence is defined by a half-open
   interval [first,last).


   ::


      T AssociativeReduce( const T* first, const T* last, T identity ) {
         return oneapi::tbb::parallel_reduce(
             // Index range for reduction
             oneapi::tbb::blocked_range<const T*>(first,last),
             // Identity element
             identity,
             // Reduce a subrange and partial sum
             [&]( oneapi::tbb::blocked_range<const T*> r, T partial_sum )->float {
                 return std::accumulate( r.begin(), r.end(), partial_sum );
             },
             // Reduce two partial sums
             std::plus<T>()
         );
      }


   The third and fourth arguments to this form of ``parallel_reduce``
   are a built in form of the agglomeration pattern. If there is an
   elementwise action to be performed before the reduction,
   incorporating it into the third argument (reduction of a subrange)
   may improve performance because of better locality of reference. Note
   that the block size for agglomeration is not explicitly specified;
   ``parallel_reduce`` defines blocks automatically with the help of
   implicitly used ``oneapi::tbb::auto_partitioner``.


   The second example assumes the + is commutative on ``T``. It is a
   good solution when ``T`` objects are expensive to construct.


   ::


      T CombineReduce( const T* first, const T* last, T identity ) {
         oneapi::tbb::combinable<T> sum(identity);
         oneapi::tbb::parallel_for(
             oneapi::tbb::blocked_range<const T*>(first,last),
             [&]( oneapi::tbb::blocked_range<const T*> r ) {
                 sum.local() += std::accumulate(r.begin(), r.end(), identity);
             }
         );
         return sum.combine( []( const T& x, const T& y ) {return x+y;} );
      }


   Sometimes it is desirable to destructively use the partial results to
   generate the final result. For example, if the partial results are
   lists, they can be spliced together to form the final result. In that
   case use class ``oneapi::tbb::enumerable_thread_specific`` instead of
   ``combinable``. The ``ParallelFindCollisions`` example in :ref:`Divide_and_Conquer`
   demonstrates the technique.


   Floating-point addition and multiplication are almost associative.
   Reassociation can cause changes because of rounding effects. The
   techniques shown so far reassociate terms non-deterministically.
   Fully deterministic parallel reduction for a not quite associative
   operation requires using deterministic reassociation. The code below
   demonstrates this in the form of a template that does a + reduction
   over a sequence of values of type ``T``.


   ::


      template<typename T>
      T RepeatableReduce( const T* first, const T* last, T identity ) {
         if( last-first<=1000 ) {
             // Use serial reduction
             return std::accumulate( first, last, identity );
         } else {
             // Do parallel divide-and-conquer reduction
             const T* mid = first+(last-first)/2;
             T left, right;
             oneapi::tbb::parallel_invoke(
                 [&]{left=RepeatableReduce(first,mid,identity);},
                 [&]{right=RepeatableReduce(mid,last,identity);} 
             );
             return left+right;
         }
      }


   The outer if-else is an instance of the agglomeration pattern for
   recursive computations. The reduction graph, though not a strict
   binary tree, is fully deterministic. Thus the result will always be
   the same for a given input sequence, assuming all threads do
   identical floating-point rounding.


   ``oneapi::tbb::parallel_deterministic_reduce`` is a simpler and more
   efficient way to get reproducible non-associative reduction. It is
   very similar to ``oneapi::tbb::parallel_reduce`` but, unlike the latter,
   builds a deterministic reduction graph. With it, the
   ``RepeatableReduce`` sample can be almost identical to
   ``AssociativeReduce``:


   ::


      template<typename T>
      T RepeatableReduce( const T* first, const T* last, T identity ) {
         return oneapi::tbb::parallel_deterministic_reduce(
             // Index range for reduction
             oneapi::tbb::blocked_range<const T*>(first,last,1000),
             // Identity element
             identity,
             // Reduce a subrange and partial sum
             [&]( oneapi::tbb::blocked_range<const T*> r, T partial_sum )->float {
                 return std::accumulate( r.begin(), r.end(), partial_sum );
             },
             // Reduce two partial sums
             std::plus<T>()
         );
      }


   Besides the function name change, note the grain size of 1000
   specified for ``oneapi::tbb::blocked_range``. It defines the desired block
   size for agglomeration; automatic block size selection is not used
   due to non-determinism.


   The final example shows how a problem that typically is not viewed as
   a reduction can be parallelized by viewing it as a reduction. The
   problem is retrieving floating-point exception flags for a
   computation across a data set. The serial code might look something
   like:


   ::


         feclearexcept(FE_ALL_EXCEPT);
         for( int i=0; i<N; ++i )
             C[i]=A[i]*B[i];
         int flags = fetestexcept(FE_ALL_EXCEPT);
         if (flags & FE_DIVBYZERO) ...;
         if (flags & FE_OVERFLOW) ...;
         ...


   The code can be parallelized by computing chunks of the loop
   separately, and merging floating-point flags from each chunk. To do
   this with ``tbb:parallel_reduce``, first define a "body" type, as
   shown below.


   ::


      struct ComputeChunk {
         int flags;          // Holds floating-point exceptions seen so far.
         void reset_fpe() {
             flags=0;
             feclearexcept(FE_ALL_EXCEPT);
         }
         ComputeChunk () {
             reset_fpe();
         }
         // "Splitting constructor"called by parallel_reduce when splitting a range into subranges.
         ComputeChunk ( const ComputeChunk&, oneapi::tbb::split ) {
             reset_fpe();
         }
         // Operates on a chunk and collects floating-point exception state into flags member.
         void operator()( oneapi::tbb::blocked_range<int> r ) {
             int end=r.end();
             for( int i=r.begin(); i!=end; ++i )
                 C[i] = A[i]/B[i];
             // It is critical to do |= here, not =, because otherwise we
             // might lose earlier exceptions from the same thread.
             flags |= fetestexcept(FE_ALL_EXCEPT);
         }
         // Called by parallel_reduce when joining results from two subranges.
         void join( Body& other ) {
             flags |= other.flags;
         }
      };


   Then invoke it as follows:


   ::


      // Construction of cc implicitly resets FP exception state.
         ComputeChunk cc;
         oneapi::tbb::parallel_reduce( oneapi::tbb::blocked_range<int>(0,N), cc );
         if (cc.flags & FE_DIVBYZERO) ...;
         if (cc.flags & FE_OVERFLOW) ...;
         ...

