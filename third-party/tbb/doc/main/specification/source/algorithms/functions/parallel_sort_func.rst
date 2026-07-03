.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
parallel_sort
=============
**[algorithms.parallel_sort]**

Function template that sorts a sequence.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_sort.h>

    namespace oneapi {
        namespace tbb {

            template<typename RandomAccessIterator>
            void parallel_sort( RandomAccessIterator begin, RandomAccessIterator end );
            template<typename RandomAccessIterator, typename Compare>
            void parallel_sort( RandomAccessIterator begin, RandomAccessIterator end, const Compare& comp );

            template<typename Container>
            void parallel_sort( Container&& c );
            template<typename Container>
            void parallel_sort( Container&& c, const Compare& comp );

        } //  namespace tbb
    } // namespace oneapi

Requirements:

* The ``RandomAccessIterator`` type must meet the `Random Access Iterators` requirements from
  [random.access.iterators] and `ValueSwappable` requirements from the [swappable.requirements] ISO C++ Standard section.
* The ``Compare`` type must meet the `Compare` type requirements from the [alg.sorting] ISO C++ Standard section.
* The ``Container`` type must meet the :doc:`ContainerBasedSequence requirements <../../named_requirements/algorithms/container_based_sequence>` 
  which iterators must meet the `Random Access Iterators` requirements from [random.access.iterators]  
  and `Swappable` requirements from the [swappable.requirements] ISO C++ Standard section.
* The type of dereferenced ``RandomAccessIterator`` or dereferenced ``Container`` iterator must meet the MoveAssignable requirements from [moveassignable] section of ISO C++ Standard and the MoveConstructible requirements from [moveconstructible] section of ISO C++ Standard.

Sorts a sequence or a container. The sort is neither stable nor deterministic: relative
ordering of elements with equal keys is not preserved and not guaranteed to repeat if the same
sequence is sorted again.

A call ``parallel_sort( begin, end, comp )`` sorts the sequence *[begin, end)* using the argument 
``comp`` to determine relative orderings.  If ``comp( x, y )`` returns ``true``, *x* appears before
*y* in the sorted sequence.

A call ``parallel_sort( begin, end )`` is equivalent to ``parallel_sort( begin, end, comp )``, where `comp`
uses `operator<` to determine relative orderings.

A call ``parallel_sort( c, comp )`` is equivalent to ``parallel_sort( std::begin(c), std::end(c), comp )``.

A call ``parallel_sort( c )`` is equivalent to ``parallel_sort( c, comp )``, where `comp` uses `operator<`
to determine relative orderings.

**Complexity**

``parallel_sort`` is a comparison sort with an average time complexity of *O(N×log(N))*, where *N* is
the number of elements in the sequence. ``parallel_sort`` may be executed concurrently to improve execution time.
