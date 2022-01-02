.. _parallel_for:

parallel_for
============


Suppose you want to apply a function ``Foo`` to each element of an
array, and it is safe to process each element concurrently. Here is the
sequential code to do this:


::


   void SerialApplyFoo( float a[], size_t n ) {
       for( size_t i=0; i!=n; ++i )
           Foo(a[i]);
   }


The iteration space here is of type ``size_t``, and goes from ``0`` to
``n-1``. The template function ``oneapi::tbb::parallel_for`` breaks this iteration
space into chunks, and runs each chunk on a separate thread. The first
step in parallelizing this loop is to convert the loop body into a form
that operates on a chunk. The form is an STL-style function object,
called the *body* object, in which ``operator()`` processes a chunk. The
following code declares the body object.

::

   #include "oneapi/tbb.h"

   using namespace oneapi::tbb;

   class ApplyFoo {
       float *const my_a;
   public:
       void operator()( const blocked_range<size_t>& r ) const {
           float *a = my_a;
           for( size_t i=r.begin(); i!=r.end(); ++i ) 
              Foo(a[i]);
       }
       ApplyFoo( float a[] ) :
           my_a(a)
       {}
   };


The ``using`` directive in the example enables you to use the library
identifiers without having to write out the namespace prefix ``oneapi::tbb``
before each identifier. The rest of the examples assume that such a
``using`` directive is present.


Note the argument to ``operator()``. A ``blocked_range<T>`` is a
template class provided by the library. It describes a one-dimensional
iteration space over type ``T``. Class ``parallel_for`` works with other
kinds of iteration spaces too. The library provides ``blocked_range2d``
for two-dimensional spaces. You can define your own spaces as explained
in :ref:`Advanced_Topic_Other_Kinds_of_Iteration_Spaces`.


An instance of ``ApplyFoo`` needs member fields that remember all the
local variables that were defined outside the original loop but used
inside it. Usually, the constructor for the body object will initialize
these fields, though ``parallel_for`` does not care how the body object
is created. Template function ``parallel_for`` requires that the body
object have a copy constructor, which is invoked to create a separate
copy (or copies) for each worker thread. It also invokes the destructor
to destroy these copies. In most cases, the implicitly generated copy
constructor and destructor work correctly. If they do not, it is almost
always the case (as usual in C++) that you must define *both* to be
consistent.


Because the body object might be copied, its ``operator()`` should not
modify the body. Otherwise the modification might or might not become
visible to the thread that invoked ``parallel_for``, depending upon
whether ``operator()`` is acting on the original or a copy. As a
reminder of this nuance, ``parallel_for`` requires that the body
object's ``operator()`` be declared ``const``.


The example ``operator()`` loads ``my_a`` into a local variable ``a``.
Though not necessary, there are two reasons for doing this in the
example:


-  **Style**. It makes the loop body look more like the original.


-  **Performance**. Sometimes putting frequently accessed values into
   local variables helps the compiler optimize the loop better, because
   local variables are often easier for the compiler to track.


Once you have the loop body written as a body object, invoke the
template function ``parallel_for``, as follows:


::


   #include "oneapi/tbb.h"
    

   void ParallelApplyFoo( float a[], size_t n ) {
       parallel_for(blocked_range<size_t>(0,n), ApplyFoo(a));
   }


The ``blocked_range`` constructed here represents the entire iteration
space from 0 to n-1, which ``parallel_for`` divides into subspaces for
each processor. The general form of the constructor is
``blocked_range<T>(begin,end,grainsize)``. The ``T`` specifies the value
type. The arguments ``begin`` and ``end`` specify the iteration space
STL-style as a half-open interval [``begin``,\ ``end``). The argument
*grainsize* is explained in the :ref:`Controlling_Chunking` section. The
example uses the default grainsize of 1 because by default
``parallel_for`` applies a heuristic that works well with the default
grainsize.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Lambda_Expressions
   ../tbb_userguide/Automatic_Chunking
   ../tbb_userguide/Controlling_Chunking
   ../tbb_userguide/Bandwidth_and_Cache_Affinity
   ../tbb_userguide/Partitioner_Summary
