.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
blocked_range
=============
**[algorithms.blocked_range]**

Class template for a recursively divisible half-open interval.

A ``blocked_range`` represents a half-open range [*i*,*j*) that can be recursively split.

A ``blocked_range`` meets the :doc:`Range requirements <../../named_requirements/algorithms/range>`.

A ``blocked_range`` specifies a *grain size* of type ``size_t``.

A ``blocked_range`` is splittable into two subranges if the size of the range exceeds its grain size.
The ideal grain size depends on the context of the ``blocked_range``, which is typically passed as the range argument
to the loop templates ``parallel_for``, ``parallel_reduce``, or ``parallel_scan``.

.. code:: cpp

    // Defined in header <oneapi/tbb/blocked_range.h>
    
    namespace oneapi {
        namespace tbb {

            template<typename Value>
            class blocked_range {
            public:
                // types
                using size_type = size_t;
                using const_iterator = Value;

                // constructors
                blocked_range( Value begin, Value end, size_type grainsize=1 );
                blocked_range( blocked_range& r, split );
                blocked_range( blocked_range& r, proportional_split& proportion );

                // capacity
                size_type size() const;
                bool empty() const;

                // access
                size_type grainsize() const;
                bool is_divisible() const;

                // iterators
                const_iterator begin() const;
                const_iterator end() const;
            };

        } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Value`` type must meet the :doc:`BlockedRangeValue requirements <../../named_requirements/algorithms/blocked_range_val>`.

Member functions
----------------

.. cpp:type:: size_type

    The type for measuring the size of a ``blocked_range``. The type is always a ``size_t``.

.. cpp:type:: const_iterator

    The type of a value in the range. Despite its name, the ``const_iterator`` type is not necessarily an STL
    iterator; it merely needs to meet the :doc:`BlockedRangeValue requirements <../../named_requirements/algorithms/blocked_range_val>`.
    However, it is convenient to call it ``const_iterator`` so that if it is a
    const_iterator, the ``blocked_range`` behaves like a read-only STL container.

.. cpp:function:: blocked_range( Value begin, Value end, size_type grainsize=1 )

    **Requirements**: The parameter ``grainsize`` must be positive.
    The debug version of the library raises an assertion failure if this requirement is not met.

    **Effects**: Constructs a ``blocked_range`` representing the half-open interval
    ``[begin, end)`` with the given ``grainsize``.

    **Example**: The statement ``"blocked_range<int> r(5, 14, 2);"`` constructs a range of
    ``int`` that contains the values 5 through 13 inclusive, with the grain size of 2. Afterwards,
    ``r.begin()==5`` and ``r.end()==14``.

.. cpp:function:: blocked_range( blocked_range& range, split )

    Basic splitting constructor.

    **Requirements**: ``is_divisible()`` is true.

    **Effects**: Partitions ``range`` into two subranges. The newly
    constructed ``blocked_range`` is approximately the second
    half of the original ``range``, and ``range`` is updated to be the remainder. Each
    subrange has the same ``grainsize`` as the original range.

    **Example**: Let ``r`` be a ``blocked_range`` that represents a half-open interval ``[i, j)``
    with a grain size ``g``. Running the statement ``blocked_range<int> s(r, split);``
    subsequently causes r to represent ``[i, i+(j-i)/2)`` and ``s`` to represent
    ``[i+(j-i)/2, j)``, both with grain size ``g``.

.. cpp:function:: blocked_range( blocked_range& range, proportional_split proportion )

    Proportional splitting constructor.

    **Requirements**: ``is_divisible()`` is true.

    **Effects**: Partitions ``range`` into two subranges such that the ratio of their sizes is
    close to the ratio of ``proportion.left()`` to ``proportion.right()``. The newly
    constructed ``blocked_range`` is the subrange at the right, and ``range`` is
    updated to be the subrange at the left.

    **Example**: Let ``r`` be a ``blocked_range`` that represents a half-open
    interval ``[i, j)`` with a grain size ``g``. Running the statement
    ``blocked_range<int> s(r, proportional_split(2, 3));`` subsequently causes
    ``r`` to represent ``[i, i+2*(j-i)/(2+3))`` and ``s`` to represent
    ``[i+2*(j-i)/(2+3), j)``, both with grain size ``g``.

.. cpp:function:: size_type size() const

    **Requirements**: ``end()<begin()`` is false.

    **Effects**: Determines size of range.

    **Returns**: ``end()-begin()``.

.. cpp:function:: bool empty() const

    **Effects**: Determines if range is empty.

    **Returns**: ``!(begin()<end())``

.. cpp:function:: size_type grainsize() const

    **Returns**: Grain size of range.

.. cpp:function:: bool is_divisible() const

    **Requirements**: ``end()<begin()`` is false.

    **Effects**: Determines if the range can be split into subranges.

    **Returns**: True if ``size()>grainsize()``; false, otherwise.

.. cpp:function:: const_iterator begin() const

    **Returns**: Inclusive lower bound of the range.

.. cpp:function:: const_iterator end() const

    **Returns**: Exclusive upper bound of the range.

See also:

* :doc:`parallel_reduce <../functions/parallel_reduce_func>`
* :doc:`parallel_for <../functions/parallel_for_func>`
* :doc:`parallel_scan <../functions/parallel_scan_func>`

