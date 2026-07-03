.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
proportional split
==================
**[algorithms.proportional_split]**

Type of an argument for a proportional splitting constructor of :doc:`Range <../../named_requirements/algorithms/range>`.

An argument of type ``proportional_split`` may be used by classes that satisfy
:doc:`Range requirements <../../named_requirements/algorithms/range>` to distinguish a proportional
splitting constructor from a basic splitting constructor and from a copy constructor, and to suggest a ratio in which a particular instance of
the class should be split.

.. code:: cpp

    // Defined in header <oneapi/tbb/blocked_range.h>
    // Defined in header <oneapi/tbb/blocked_range2d.h>
    // Defined in header <oneapi/tbb/blocked_range3d.h>
    // Defined in header <oneapi/tbb/blocked_nd_range.h>
    // Defined in header <oneapi/tbb/partitioner.h>
    // Defined in header <oneapi/tbb/parallel_for.h>
    // Defined in header <oneapi/tbb/parallel_reduce.h>
    // Defined in header <oneapi/tbb/parallel_scan.h>

    namespace oneapi {
        namespace tbb {
           class proportional_split {
           public:
               proportional_split(std::size_t _left = 1, std::size_t _right = 1);

               std::size_t left() const;
               std::size_t right() const;

               explicit operator split() const;
           };
        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. cpp:function:: proportional_split( std::size_t _left = 1, std::size_t _right = 1 )

    Constructs a proportion with the ratio specified by coefficients *_left* and *_right*.

.. cpp:function:: std::size_t left() const

    Returns the size of the left part of the proportion.

.. cpp:function:: std::size_t right() const

    Returns the size of the right part of the proportion.

.. cpp:function:: explicit operator split() const

    Makes ``proportional_split`` convertible to the ``split`` type to use with
    ranges that do not support proportional splitting.

See also:

* :doc:`split <split_cls>`
* :doc:`Range requirements <../../named_requirements/algorithms/range>`

