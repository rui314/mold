.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
ParallelForFunc
================
**[req.parallel_for_func]**

A type `F` satisfies `ParallelForFunc` if it meets the following requirements:

--------------------------------------------------------------------------------

**ParallelForFunc Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: void F::operator()(Index index) const

    Applies the function to the index. ``Index`` type must be the same as corresponding template parameter of the :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`.

See also:

* :doc:`parallel_for algorithm <../../algorithms/functions/parallel_for_func>`
* :doc:`ParallelForIndex named requirement <par_for_index>`
