.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
ParallelForBody
===============
**[req.parallel_for_body]**

A type `Body` satisfies `ParallelForBody` if it meets the following requirements:

----------------------------------------------------------------------

**ParallelForBody Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Body::Body( const Body& )

    Copy constructor.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: void Body::operator()( Range& range ) const

    Applies body to a range. ``Range`` type must meet the :doc:`Range requirements <range>`.

See also:

* :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`
