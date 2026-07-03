.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
ParallelForIndex
=================
**[req.parallel_for_index]**

A type `Index` satisfies `ParallelForIndex` if it meets the following requirements:

------------------------------------------------------------------------------------------

**ParallelForIndex Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Index::Index( int )

    Constructor from an ``int`` value.

.. cpp:function:: Index::Index( const Index& )

    Copy constructor.

.. cpp:function:: Index::~Index()

    Destructor.

.. cpp:function:: Index& operator=( const Index& )

    Assignment.

.. cpp:function:: Index& operator++()

    Adjust ``*this`` to the next value.

.. cpp:function:: bool operator<( const Index& i, const Index& j )

    Value of *i* precedes value of *j*.

.. cpp:function:: bool operator<=( const Index& i, const Index& j )

    Value of *i* precedes or equal to the value of *j*.

.. cpp:function:: D operator-( const Index& i, const Index& j )

    Number of values in range ``[i,j)``.

.. cpp:function:: Index operator+( const Index& i, const Index& j )

    Sum of *i* and *j* values.

.. cpp:function:: Index operator+( const Index& i, D k )

    *k*-th value after *i*.

.. cpp:function:: Index operator*( const Index& i, const Index& j )

    Multiplication of *i* and *j* values.

.. cpp:function:: Index operator/( const Index& i, const Index& j )

    Quotient of *i* and *j* values.

``D`` is the type of the expression ``j-i``. It can be any integral type that is convertible to ``size_t``.
Examples that model the Index requirements are integral types and pointers.

**_NOTE:_** It is recommended to use integral types as ``ParallelForIndex``. See the ``[basic.fundamental]``
section of the ISO C++ Standard for information about integral types.

See also:

* :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`
