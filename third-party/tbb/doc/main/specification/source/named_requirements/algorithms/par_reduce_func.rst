.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
ParallelReduceFunc
==================
**[req.parallel_reduce_func]**

A type `Func` satisfies `ParallelReduceFunc` if it meets the following requirements:

-----------------------------------------------------------------------------------------------------

**ParallelReduceFunc Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Value Func::operator()(const Range& range, Value&& x) const

    Accumulates values over ``range``, starting with the initial value ``x``.
    The ``Range`` type must meet the :doc:`Range requirements <range>`.
    The ``Value`` type must be the same as the corresponding template parameter for the
    :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`.

See also:

* :doc:`parallel_reduce algorithm <../../algorithms/functions/parallel_reduce_func>`
* :doc:`parallel_deterministic_reduce algorithm <../../algorithms/functions/parallel_deterministic_reduce_func>`
