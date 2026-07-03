.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

====================
affinity_partitioner
====================
**[algorithms.affinity_partitioner]**

Hints that loop iterations should be assigned to threads in a way that optimizes for cache affinity.

An ``affinity_partitioner`` hints that execution of a loop template should use the same task affinity pattern
for splitting the work as used by previous execution of the loop (or another loop) with the same ``affinity_partitioner`` object.

``affinity_partitioner`` uses proportional splitting when it is enabled for a :doc:`Range <../../named_requirements/algorithms/range>` type.

Unlike the other partitioners, it is important that the same ``affinity_partitioner`` object
be passed to the loop templates to be optimized for affinity.

The ``affinity_partitioner`` class satisfies the *CopyConstructibe* requirement from the ISO C++ [utility.arg.requirements] section.


.. code:: cpp

    // Defined in header <oneapi/tbb/partitioner.h>

    namespace oneapi {
        namespace tbb {

           class affinity_partitioner {
           public:
               affinity_partitioner() = default;
               ~affinity_partitioner() = default;
           };

        } // namespace tbb
    } // namespace oneapi

See also:

* :doc:`Range named requirement <../../named_requirements/algorithms/range>`

