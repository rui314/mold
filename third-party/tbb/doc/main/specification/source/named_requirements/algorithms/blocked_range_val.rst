.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
BlockedRangeValue
=================
**[req.blocked_range_value]**

A type `Value` satisfies `BlockedRangeValue` if it meets the following requirements:

------------------------------------------------------------------------------------------

**BlockedRangeValue Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Value::Value( const Value& )

    Copy constructor.

.. cpp:function:: Value::~Value()

    Destructor.

.. cpp:function:: void operator=( const Value& )

    Assignment.
    
    .. note::

         The return type ``void`` in the pseudo-signature denotes that
         ``operator=`` is not required to return a value. The actual ``operator=``
         can return a value, which will be ignored by ``blocked_range`` .

.. cpp:function:: bool operator<( const Value& i, const Value& j )

    Value *i* precedes value *j*.

.. cpp:function:: D operator-( const Value& i, const Value& j )

    Number of values in range ``[i,j)``.

.. cpp:function:: Value operator+( const Value& i, D k )

    *k*-th value after *i*.

``D`` is the type of the expression ``j-i``. It can be any integral type that is convertible to ``size_t``.
Examples that model the Value requirements are integral types, pointers, and STL random-access iterators
whose difference can be implicitly converted to a ``size_t``.

See also:

* :doc:`blocked_range class <../../algorithms/blocked_ranges/blocked_range_cls>`
* :doc:`blocked_range2d class <../../algorithms/blocked_ranges/blocked_range2d_cls>`
* :doc:`blocked_range3d class <../../algorithms/blocked_ranges/blocked_range3d_cls>`
* :doc:`blocked_nd_range class <../../algorithms/blocked_ranges/blocked_nd_range_cls>`
* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
* :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`
