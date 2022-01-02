.. _Advanced_Example:

Advanced Example
================


An example of a more advanced associative operation is to find the index
where ``Foo(i)`` is minimized. A serial version might look like this:


::


   long SerialMinIndexFoo( const float a[], size_t n ) {
       float value_of_min = FLT_MAX;        // FLT_MAX from <climits>
       long index_of_min = -1;
       for( size_t i=0; i<n; ++i ) {
           float value = Foo(a[i]);
           if( value<value_of_min ) {
               value_of_min = value;
               index_of_min = i;
           }
       }  
       return index_of_min;
   }


The loop works by keeping track of the minimum value found so far, and
the index of this value. This is the only information carried between
loop iterations. To convert the loop to use ``parallel_reduce``, the
function object must keep track of the carried information, and how to
merge this information when iterations are spread across multiple
threads. Also, the function object must record a pointer to ``a`` to
provide context.


The following code shows the complete function object.


::


   class MinIndexFoo {
       const float *const my_a;
   public:
       float value_of_min;
       long index_of_min; 
       void operator()( const blocked_range<size_t>& r ) {
           const float *a = my_a;
           for( size_t i=r.begin(); i!=r.end(); ++i ) {
              float value = Foo(a[i]);    
              if( value<value_of_min ) {
                  value_of_min = value;
                  index_of_min = i;
              }
           }
       }
    

       MinIndexFoo( MinIndexFoo& x, split ) : 
           my_a(x.my_a), 
           value_of_min(FLT_MAX),    // FLT_MAX from <climits>
           index_of_min(-1) 
      {}
    

       void join( const SumFoo& y ) {
           if( y.value_of_min<value_of_min ) {
               value_of_min = y.value_of_min;
               index_of_min = y.index_of_min;
           }
       }
                

       MinIndexFoo( const float a[] ) :
           my_a(a), 
           value_of_min(FLT_MAX),    // FLT_MAX from <climits>
           index_of_min(-1),
       {}
   };


Now ``SerialMinIndex`` can be rewritten using ``parallel_reduce`` as
shown below:


::


   long ParallelMinIndexFoo( float a[], size_t n ) {
       MinIndexFoo mif(a);
       parallel_reduce(blocked_range<size_t>(0,n), mif );
      

    return mif.index_of_min;
   }
