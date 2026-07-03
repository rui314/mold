.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
ParallelReduceBody
==================
**[req.parallel_reduce_body]**

A type `Body` satisfies `ParallelReduceBody` if it meets the following requirements:

-----------------------------------------------------------------------------------------------------

**ParallelReduceBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: ParallelReduceBody
	       
.. cpp:function:: Body::Body( Body&, split )

    Splitting constructor. Must be able to run concurrently with ``operator()`` and method ``join``.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: void Body::operator()(const Range& range)

    Accumulates result for a subrange. ``Range`` type must meet the :doc:`Range requirements <range>`.

.. cpp:function:: void Body::join( Body& rhs )

    Joins results. The result in rhs should be merged into the result of ``this``.

See also:

* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
* :doc:`parallel_deterministic_reduce algorithm <../../algorithms/functions/parallel_deterministic_reduce_func>`
