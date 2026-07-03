.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
ParallelReduceReduction
=======================
**[req.parallel_reduce_reduction]**

A type `Reduction` satisfies `ParallelReduceReduction` if it meets the following requirements:

-----------------------------------------------------------------------------------------------------

**ParallelReduceReduction Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Value Reduction::operator()(Value&& x, Value&& y) const

    Combines the results ``x`` and ``y``.
    The ``Value`` type must be the same as the corresponding template parameter for the 
    :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`.

See also:

* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
* :doc:`parallel_deterministic_reduce algorithm <../../algorithms/functions/parallel_deterministic_reduce_func>`
