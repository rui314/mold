.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
simple_partitioner
==================
**[algorithms.simple_partitioner]**

Specifies that a parallel loop should recursively split its range until it cannot be further subdivided.

A ``simple_partitioner`` specifies that a loop template should recursively divide its range
until for each subrange *r*, the condition ``!r.is_divisible()`` holds.
This is the default behavior of the loop templates that take a range argument.

The ``simple_partitioner`` class satisfies the *CopyConstructibe* requirement from the ISO C++ [utility.arg.requirements] section.

.. tip::

   When using ``simple_partitioner`` and a ``blocked_range`` for a parallel loop,
   make sure to specify an appropriate grain size for the ``blocked_range``.
   The default grain size is 1, which may make the subranges much too small for efficient execution.

.. code:: cpp

    // Defined in header <oneapi/tbb/partitioner.h>

    namespace oneapi {
        namespace tbb {

           class simple_partitioner {
           public:
               simple_partitioner() = default;
               ~simple_partitioner() = default;
           };

        } // namespace tbb
    } // namespace oneapi

See also:

* :doc:`Range named requirement <../../named_requirements/algorithms/range>`

