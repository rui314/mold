.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
ParallelScanBody
================
**[req.parallel_scan]**

A type `Body` satisfies `ParallelScanBody` if it meets the following requirements:

--------------------------------------------------------------------------------

**ParallelScanBody Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: void Body::operator()( const Range& r, pre_scan_tag )

    Accumulates summary for range ``r``.
    For example, when computing a running sum of an array,
    the summary for a range ``r`` is the sum of the array elements corresponding to ``r``.

.. cpp:function:: void Body::operator()( const Range& r, final_scan_tag )

    Computes scan result and summary for range ``r``.

.. cpp:function:: Body::Body( Body& b, split )

    Splits ``b`` so that ``this`` and ``b`` can accumulate summaries separately.

.. cpp:function:: void Body::reverse_join( Body& b )

    Merges the summary accumulated by ``b`` into the summary accumulated by ``this``,
    where ``this`` was created earlier from ``b`` by splitting constructor.

.. cpp:function:: void Body::assign( Body& b )

    Assigns summary of ``b`` to ``this``.

See also:

* :doc:`parallel_scan algorithm <../../algorithms/functions/parallel_scan_func>`
