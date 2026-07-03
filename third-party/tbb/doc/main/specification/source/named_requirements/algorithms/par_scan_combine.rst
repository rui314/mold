.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
ParallelScanCombine
===================
**[req.parallel_scan_combine]**

A type `Combine` satisfies `ParallelScanCombine` if it meets the following requirements:

--------------------------------------------------------------------------------

**ParallelScanCombine Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Value Combine::operator()(const Value& left, const Value& right) const

    Combines summaries ``left`` and ``right`` and returns the result
    ``Value`` type must be the same as a corresponding template parameter for the ``parallel_scan`` algorithm.

See also:

* :doc:`parallel_scan algorithm <../../algorithms/functions/parallel_scan_func>`
