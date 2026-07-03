.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
ParallelScanFunc
================
**[req.parallel_scan_func]**

A type `Scan` satisfies `ParallelScanFunc` if it meets the following requirements:

--------------------------------------------------------------------------------

**ParallelScanFunc Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Value Scan::operator()(const Range& r, const Value& sum, bool is_final) const

    Starting with ``sum``, computes the summary and, for ``is_final == true``,
    the scan result for range ``r``. Returns the computed summary.
    ``Value`` type must be the same as a corresponding template parameter for the ``parallel_scan`` algorithm.

See also:

* :doc:`parallel_scan algorithm <../../algorithms/functions/parallel_scan_func>`
