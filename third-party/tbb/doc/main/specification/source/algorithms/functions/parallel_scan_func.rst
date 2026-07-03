.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
parallel_scan
=============
**[algorithms.parallel_scan]**

Function template that computes a parallel prefix.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_scan.h>

    template<typename Range, typename Body>
    void parallel_scan( const Range& range, Body& body );
    template<typename Range, typename Body>
    void parallel_scan( const Range& range, Body& body, /* see-below */ partitioner );

    template<typename Range, typename Value, typename Scan, typename Combine>
    Value parallel_scan( const Range& range, const Value& identity, const Scan& scan, const Combine& combine );
    template<typename Range, typename Value, typename Scan, typename Combine>
    Value parallel_scan( const Range& range, const Value& identity, const Scan& scan, const Combine& combine, /* see-below */ partitioner );

A ``partitioner`` type may be one of the following entities:

* ``const auto_partitioner&``
* ``const simple_partitioner&``

Requirements:

* The ``Range`` type must meet the :doc:`Range requirement <../../named_requirements/algorithms/range>`.
* The ``Body`` type must meet the :doc:`ParallelScanBody requirements <../../named_requirements/algorithms/par_scan_body>`.
* The ``Value`` type must meet the `CopyConstructible` requirements from the [copyconstructible] section and
  `CopyAssignable` requirements from the [copyassignable] section of the ISO C++ Standard.
* The ``Scan`` type must meet the :doc:`ParallelScanFunc requirements <../../named_requirements/algorithms/par_scan_func>`.
  Since C++17, ``Scan`` may also be a pointer to a const member function in ``Range`` that takes ``const Value&`` and ``bool`` arguments and
  returns ``Value``.
* The ``Combine`` type must meet the :doc:`ParallelScanCombine requirements <../../named_requirements/algorithms/par_scan_combine>`.
  Since C++17, ``Combine`` may also be a pointer to a const member function in ``Value`` that takes ``const Value&`` argument
  and returns ``Value``.

The function template ``parallel_scan`` computes a parallel prefix, also known as a parallel scan.
This computation is an advanced concept in parallel computing that is sometimes useful in scenarios
that appear to have inherently serial dependences.

A mathematical definition of the parallel prefix is as follows. Let × be an associative operation with left-identity element id\ :sub:`×`.
The parallel prefix of × over a sequence *z*\ :sub:`0`, *z*\ :sub:`1`, ...*z*\ :sub:`n-1`
is a sequence *y*\ :sub:`0`, *y*\ :sub:`1`, *y*\ :sub:`2`, ...*y*\ :sub:`n-1` where:

* y\ :sub:`0` = id\ :sub:`×` × z\ :sub:`0`
* y\ :sub:`i` = y\ :sub:`i-1` × z\ :sub:`i`

For example, if × is addition, the parallel prefix corresponds to a running sum.
A serial implementation of a parallel prefix is:

.. code:: cpp

    T temp = id;
    for( int i=1; i<=n; ++i ) {
       temp = temp + z[i];
       y[i] = temp;
    }

Parallel prefix performs this in parallel by reassociating the application of × (``+`` in example) and using two passes.
It may invoke × up to twice as many times as the serial prefix algorithm.
Even though it does more work, given the right grain size the parallel algorithm can outperform the serial one
because it distributes the work across multiple hardware threads.

The function template ``parallel_scan`` has two forms.
The imperative form ``parallel_scan(range, body)`` implements parallel prefix generically.

A summary (refer to :doc:`ParallelScanBody requirements <../../named_requirements/algorithms/par_scan_body>`)
contains enough information such that for two consecutive subranges *r* and *s*:

* If *r* has no preceding subrange, the scan result for *s* can be computed from knowing *s* and the summary for *r*.
* A summary of *r* concatenated with *s* can be computed from the summaries of *r* and *s*.

The functional form ``parallel_scan(range, identity, scan, combine)`` is designed
to use with functors and lambda expressions, hiding some complexities of the imperative form.
It uses the same *scan* functor in both passes, differentiating them via a boolean parameter,
combines summaries with *combine* functor, and returns the summary computed over the whole *range*.
The *identity* argument is the left identity element for ``Scan::operator()``.

pre_scan and final_scan Classes
-------------------------------

.. toctree::
    :titlesonly:

    pre_scan_tag_and_final_scan_tag_clses.rst

The ``parallel_scan`` template makes an effort to avoid prescanning where possible.
When executed serially, ``parallel_scan`` processes the subranges without any pre-scans
by processing the subranges from left to right using final scans.
That is why final scans must compute a summary as well as the final scan result.
The summary might be needed to process the next subrange if no other thread has pre-scanned it yet.

Example (Imperative Form)
-------------------------

The following code demonstrates how ``Body`` could be implemented for ``parallel_scan``
to compute the same result as in the earlier sequential example.

.. code:: cpp

   class Body {
       T sum;
       T* const y;
       const T* const z;
   public:
       Body( T y_[], const T z_[] ) : sum(id), z(z_), y(y_) {}
       T get_sum() const { return sum; }

       template<typename Tag>
       void operator()( const oneapi::tbb::blocked_range<int>& r, Tag ) {
           T temp = sum;
           for( int i=r.begin(); i<r.end(); ++i ) {
               temp = temp + z[i];
               if( Tag::is_final_scan() )
                   y[i] = temp;
           }
           sum = temp;
       }
       Body( Body& b, oneapi::tbb::split ) : z(b.z), y(b.y), sum(id) {}
       void reverse_join( Body& a ) { sum = a.sum + sum; }
       void assign( Body& b ) { sum = b.sum; }
   };

   T DoParallelScan( T y[], const T z[], int n ) {
       Body body(y,z);
       oneapi::tbb::parallel_scan( oneapi::tbb::blocked_range<int>(0,n), body );
       return body.get_sum();
   }

The definition of ``operator()`` demonstrates typical patterns when using ``parallel_scan``.

* A single template defines both versions. Doing so is not required, but usually saves coding
  effort, because two versions are usually similar. The library defines the static method
  ``is_final_scan`` to enable differentiation between the versions.
* The prescan variant computes the × reduction, but does not update ``y``.
  The prescan is used by ``parallel_scan`` to generate look-ahead partial reductions.
* The final scan variant computes the × reduction and updates ``y``.

The ``reverse_join`` operation is similar to the ``join`` operation used by ``parallel_reduce``, except that the arguments are reversed.
That is, ``this`` is the *right* argument of ×. The template function ``parallel_scan`` decides if and when to generate parallel work.
Thus, it is crucial that × is associative and that the methods of ``Body`` faithfully represent it.
Operations such as floating-point addition, which are somewhat associative, can be used with the
understanding that the results may be rounded differently depending on the association used by ``parallel_scan``.
The reassociation may differ between runs even on the same machine.
However, when executed serially, ``parallel_scan`` associates identically to the serial form shown at the beginning of this section.

If you change the example to use a ``simple_partitioner``, be sure to provide a grain size.
The code below shows how to do this for the grain size of 1000:

.. code:: cpp

   parallel_scan( blocked_range<int>(0,n,1000), total, simple_partitioner() );

Example with Lambda Expressions
-------------------------------

The following is analogous to the previous example,
but written using lambda expressions and the functional form of ``parallel_scan``:

.. code:: cpp

    T DoParallelScan( T y[], const T z[], int n ) {
        return oneapi::tbb::parallel_scan(
            oneapi::tbb::blocked_range<int>(0,n),
            id,
            [](const oneapi::tbb::blocked_range<int>& r, T sum, bool is_final_scan)->T {
                T temp = sum;
                for( int i=r.begin(); i<r.end(); ++i ) {
                    temp = temp + z[i];
                    if( is_final_scan )
                        y[i] = temp;
                }
                return temp;
            },
            []( T left, T right ) {
                return left + right;
            }
        );
    }

See also:

* :doc:`blocked_range class <../../algorithms/blocked_ranges/blocked_range_cls>`
* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
