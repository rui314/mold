.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====
Range
=====
**[req.range]**

A `Range` can be recursively subdivided into two parts. Subdivision is done by calling
*splitting constructor* of a `Range`. There are two types of splitting constructors:

* Basic splitting constructor. In this constructor, it is recommended that the division is done into nearly equal parts, but it is not required. Splitting as evenly as possible typically yields
  the best parallelism.
* Proportional splitting constructor. This constructor is optional and can be omitted.
  When using this type of constructor, for the best results, follow the given proportion
  with rounding to the nearest integer if necessary.

Ideally, a range is recursively splittable until the parts represent portions of work that
are more efficient to execute serially rather than split further. The amount of work represented
by `Range` typically depends on higher level context, therefore a typical type that models a
`Range` should provide a way to control the degree of splitting. For example, the template class
:doc:`blocked_range <../../algorithms/blocked_ranges/blocked_range_cls>`
has the *grainsize* parameter that specifies the biggest range considered indivisible.

If the set of values has a sense of direction, by convention the splitting constructor
should construct the second part of the range and update its argument to be the first part
of the range. This causes the :doc:`parallel_for <../../algorithms/functions/parallel_for_func>`,
:doc:`parallel_reduce <../../algorithms/functions/parallel_reduce_func>`, and
:doc:`parallel_scan <../../algorithms/functions/parallel_scan_func>` algorithms,
when running sequentially, to work across a range in the increasing order, which is typical of an ordinary sequential loop.

Because a `Range` declares splitting and copy constructors, the default constructor for it is not generated automatically. 
You need to explicitly define the default constructor or add any
other constructor to create an instance of a `Range` type in the program.

A type `R` meets `Range` if it satisfies the following requirements:

-----------------------------------------------------

**Range Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: R::R( const R& )

    Copy constructor.

.. cpp:function:: R::~R()

    Destructor.

.. cpp:function:: bool R::empty() const

    True if range is empty.

.. cpp:function:: bool R::is_divisible() const

    True if range can be partitioned into two subranges.

.. cpp:function:: R::R( R& r, split )

    Basic splitting constructor. Splits ``r`` into two subranges.

.. cpp:function:: R::R( R& r, proportional_split proportion )

    **Optional**. Proportional splitting constructor. Splits ``r`` into two subranges in accordance with ``proportion``.

See also:

* :doc:`blocked_range class <../../algorithms/blocked_ranges/blocked_range_cls>`
* :doc:`blocked_range2d class <../../algorithms/blocked_ranges/blocked_range2d_cls>`
* :doc:`blocked_range3d class <../../algorithms/blocked_ranges/blocked_range3d_cls>`
* :doc:`blocked_nd_range class <../../algorithms/blocked_ranges/blocked_nd_range_cls>`
* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
* :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`
* :doc:`split class <../../algorithms/split_tags/split_cls>`

